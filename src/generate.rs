use std::ops::Deref;

use anyhow::{Result, bail};
use askama::Template;

use crate::cli::GenArgs;
use crate::model::{EnvVar, GenContext, GenKind, Label, Volume, VolumeMount};
use crate::util;

/// Every manifest template shares the same [`GenContext`], so instead of
/// duplicating ~50 fields per kind we declare a thin wrapper that `Deref`s to
/// the context. Askama resolves root fields through Rust field access, which
/// autoderefs, so templates can keep writing `{{ name }}`.
macro_rules! manifest_template {
    ($name:ident, $path:literal) => {
        #[derive(Template)]
        #[template(path = $path)]
        pub struct $name<'a> {
            ctx: &'a GenContext,
        }

        impl<'a> Deref for $name<'a> {
            type Target = GenContext;

            fn deref(&self) -> &Self::Target {
                self.ctx
            }
        }
    };
}

manifest_template!(DeploymentTpl, "deployment.yaml");
manifest_template!(StatefulSetTpl, "statefulset.yaml");
manifest_template!(DaemonSetTpl, "daemonset.yaml");
manifest_template!(JobTpl, "job.yaml");
manifest_template!(CronJobTpl, "cronjob.yaml");
manifest_template!(ServiceTpl, "service.yaml");
manifest_template!(ConfigMapTpl, "configmap.yaml");
manifest_template!(SecretTpl, "secret.yaml");
manifest_template!(IngressTpl, "ingress.yaml");
manifest_template!(PvcTpl, "pvc.yaml");
manifest_template!(NamespaceTpl, "namespace.yaml");
manifest_template!(HpaTpl, "hpa.yaml");
manifest_template!(NetworkPolicyTpl, "networkpolicy.yaml");
manifest_template!(ServiceAccountTpl, "serviceaccount.yaml");

/// Render a manifest of `kind` from `ctx`.
pub fn render(kind: GenKind, ctx: &GenContext) -> Result<String> {
    let rendered = match kind {
        GenKind::Deployment => DeploymentTpl { ctx }.render(),
        GenKind::StatefulSet => StatefulSetTpl { ctx }.render(),
        GenKind::DaemonSet => DaemonSetTpl { ctx }.render(),
        GenKind::Job => JobTpl { ctx }.render(),
        GenKind::CronJob => CronJobTpl { ctx }.render(),
        GenKind::Service => ServiceTpl { ctx }.render(),
        GenKind::ConfigMap => ConfigMapTpl { ctx }.render(),
        GenKind::Secret => SecretTpl { ctx }.render(),
        GenKind::Ingress => IngressTpl { ctx }.render(),
        GenKind::Pvc => PvcTpl { ctx }.render(),
        GenKind::Namespace => NamespaceTpl { ctx }.render(),
        GenKind::Hpa => HpaTpl { ctx }.render(),
        GenKind::NetworkPolicy => NetworkPolicyTpl { ctx }.render(),
        GenKind::ServiceAccount => ServiceAccountTpl { ctx }.render(),
    }?;

    Ok(util::ensure_trailing_newline(rendered))
}

/// Entry point for `kute gen`.
pub fn run(args: GenArgs) -> Result<()> {
    let Some(kind) = args.kind else {
        print_kinds();
        return Ok(());
    };

    if args.list {
        print_kinds();
        return Ok(());
    }

    let Some(name) = args.name.as_deref() else {
        bail!(
            "a NAME is required, e.g. `kute gen {} my-app`",
            kind.as_str()
        );
    };

    let ctx = build_context(kind, name, &args)?;
    let manifest = render(kind, &ctx)?;
    util::emit(&manifest, args.output.as_deref(), args.force)
}

fn print_kinds() {
    println!("Supported kinds for `kute gen <KIND> <NAME>`:\n");
    for kind in GenKind::ALL {
        println!("  {:<14} {}", kind.as_str(), kind.description());
    }
    println!("\nRun `kute gen <KIND> --help` for the flags that kind understands.");
}

/// Translate CLI arguments into a template context, filling in conventional
/// labels and validating per-kind requirements.
pub fn build_context(kind: GenKind, name: &str, args: &GenArgs) -> Result<GenContext> {
    let mut ctx = GenContext::new(name);

    if kind != GenKind::Namespace {
        ctx.namespace = args.namespace.clone();
    }

    for (key, value) in &args.labels {
        upsert_label(&mut ctx.labels, key, value);
    }
    ctx.annotations = args
        .annotations
        .iter()
        .map(|(k, v)| Label::new(k, v))
        .collect();

    // Keep the selector aligned with the name label even if the user overrode it.
    if let Some(label) = ctx.labels.iter().find(|l| l.key == ctx.selector_key) {
        ctx.selector_value = label.value.clone();
    }

    ctx.replicas = args.replicas;
    ctx.image = args.image.clone();
    ctx.image_pull_policy = args.image_pull_policy.clone();
    ctx.port = args.port;
    ctx.port_name = args.port_name.clone();
    ctx.service_port = args.service_port.unwrap_or(args.port);
    ctx.target_port = args.target_port.unwrap_or(args.port);
    ctx.service_type = args.service_type.clone();
    ctx.service_account = args.service_account.clone();

    ctx.cpu_request = args.cpu_request.clone();
    ctx.cpu_limit = args.cpu_limit.clone();
    ctx.mem_request = args.mem_request.clone();
    ctx.mem_limit = args.mem_limit.clone();

    ctx.command = args.command.clone();
    ctx.args = args.args.clone();
    ctx.env = build_env(args);

    ctx.data = args.data.iter().map(|(k, v)| Label::new(k, v)).collect();
    if let Some(secret_type) = &args.secret_type {
        ctx.secret_type = secret_type.clone();
    }

    ctx.host = args.host.clone().unwrap_or_default();
    ctx.path = args.path.clone();
    ctx.path_type = args.path_type.clone();
    ctx.ingress_class = args.ingress_class.clone();
    ctx.tls_secret = args.tls_secret.clone();

    if let Some(schedule) = &args.schedule {
        ctx.schedule = schedule.clone();
    }
    ctx.restart_policy = args.restart_policy.clone();
    ctx.backoff_limit = args.backoff_limit;
    ctx.completions = args.completions;
    ctx.parallelism = args.parallelism;
    ctx.suspend = args.suspend;

    ctx.storage = args.storage.clone();
    ctx.access_mode = args.access_mode.clone();
    ctx.storage_class = args.storage_class.clone();

    ctx.min_replicas = args.min_replicas;
    ctx.max_replicas = args.max_replicas;
    ctx.target_cpu = args.target_cpu;

    ctx.automount = !args.no_automount;
    ctx.image_pull_secrets = args.image_pull_secrets.clone();

    build_volumes(&mut ctx, args);
    ctx.netpol_ports = args.netpol_port.clone();

    validate(kind, &ctx)?;
    Ok(ctx)
}

fn build_env(args: &GenArgs) -> Vec<EnvVar> {
    let mut env: Vec<EnvVar> = args
        .env
        .iter()
        .map(|(k, v)| EnvVar::literal(k, v))
        .collect();

    for (name, config_map) in &args.env_from_configmap {
        env.push(EnvVar {
            name: name.clone(),
            value: None,
            config_map_ref: Some(config_map.clone()),
            secret_ref: None,
        });
    }

    for (name, secret) in &args.env_from_secret {
        env.push(EnvVar {
            name: name.clone(),
            value: None,
            config_map_ref: None,
            secret_ref: Some(secret.clone()),
        });
    }

    env
}

fn build_volumes(ctx: &mut GenContext, args: &GenArgs) {
    for (source, mount_path) in &args.mount_configmap {
        add_volume(ctx, source, mount_path, "configMap", false);
    }
    for (source, mount_path) in &args.mount_secret {
        add_volume(ctx, source, mount_path, "secret", true);
    }
}

fn add_volume(ctx: &mut GenContext, source: &str, mount_path: &str, kind: &str, read_only: bool) {
    let volume_name = sanitize_volume_name(source);
    ctx.volumes.push(Volume {
        name: volume_name.clone(),
        kind: kind.to_string(),
        source: source.to_string(),
    });
    ctx.mounts.push(VolumeMount {
        name: volume_name,
        mount_path: mount_path.to_string(),
        read_only,
    });
}

/// Volume names must be valid DNS-1123 labels.
fn sanitize_volume_name(source: &str) -> String {
    source
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            _ => '-',
        })
        .collect()
}

fn upsert_label(labels: &mut Vec<Label>, key: &str, value: &str) {
    match labels.iter_mut().find(|l| l.key == key) {
        Some(existing) => existing.value = value.to_string(),
        None => labels.push(Label::new(key, value)),
    }
}

fn validate(kind: GenKind, ctx: &GenContext) -> Result<()> {
    if kind == GenKind::Ingress && ctx.host.is_empty() {
        bail!("ingress requires a host: `kute gen ingress <NAME> --host app.example.com`");
    }
    Ok(())
}
