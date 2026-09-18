use std::ops::Deref;
use std::path::PathBuf;

use anyhow::Result;
use askama::Template;

use crate::cli::ScaffoldArgs;
use crate::generate;
use crate::model::{GenContext, GenKind};
use crate::util;

#[derive(Template)]
#[template(path = "kustomization_base.yaml")]
struct KustomizationBaseTpl<'a> {
    ctx: &'a KustomizeBaseContext,
}

impl<'a> Deref for KustomizationBaseTpl<'a> {
    type Target = KustomizeBaseContext;

    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

pub struct KustomizeBaseContext {
    pub name: String,
    pub resources: Vec<String>,
}

#[derive(Template)]
#[template(path = "kustomization_overlay.yaml")]
struct KustomizationOverlayTpl<'a> {
    ctx: &'a KustomizeOverlayContext,
}

impl<'a> Deref for KustomizationOverlayTpl<'a> {
    type Target = KustomizeOverlayContext;

    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

pub struct KustomizeOverlayContext {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub new_tag: String,
    pub replicas: u32,
    pub env: String,
}

struct PlannedFile {
    path: PathBuf,
    content: String,
}

/// Split `registry/name:tag` into its repository and tag parts, tolerating
/// registry hosts that carry a port (e.g. `localhost:5000/app`).
fn split_image(image: &str) -> (String, String) {
    match image.rfind(':') {
        Some(idx) if !image[idx + 1..].contains('/') => {
            (image[..idx].to_string(), image[idx + 1..].to_string())
        }
        _ => (image.to_string(), "latest".to_string()),
    }
}

fn plan(args: &ScaffoldArgs) -> Result<Vec<PlannedFile>> {
    let root = args.dir.join(&args.name);
    let base = root.join("base");

    let mut workload = GenContext::new(&args.name);
    workload.image = args.image.clone();
    workload.port = args.port;
    workload.service_port = args.port;
    workload.target_port = args.port;
    workload.replicas = 1;

    let mut service = GenContext::new(&args.name);
    service.port = args.port;
    service.service_port = args.port;
    service.target_port = args.port;

    // Ingress and HPA are opt-in, so they are only added to the base when asked
    // for -- and only then do they appear in the base kustomization.
    let mut resources = vec!["deployment.yaml".to_string(), "service.yaml".to_string()];
    let mut manifests = Vec::new();

    if let Some(host) = &args.ingress_host {
        let mut ingress = GenContext::new(&args.name);
        ingress.host = host.clone();
        ingress.ingress_class = args.ingress_class.clone();
        ingress.tls_secret = args.ingress_tls_secret.clone();
        ingress.service_port = args.port;

        resources.push("ingress.yaml".to_string());
        manifests.push(PlannedFile {
            path: base.join("ingress.yaml"),
            content: generate::render(GenKind::Ingress, &ingress)?,
        });
    }

    if args.hpa {
        let mut hpa = GenContext::new(&args.name);
        hpa.min_replicas = args.hpa_min;
        hpa.max_replicas = args.hpa_max;
        hpa.target_cpu = args.hpa_cpu;

        resources.push("hpa.yaml".to_string());
        manifests.push(PlannedFile {
            path: base.join("hpa.yaml"),
            content: generate::render(GenKind::Hpa, &hpa)?,
        });
    }

    let base_kustomization = KustomizationBaseTpl {
        ctx: &KustomizeBaseContext {
            name: args.name.clone(),
            resources,
        },
    }
    .render()?;

    let (image_name, image_tag) = split_image(&args.image);
    let new_tag = args.tag.clone().unwrap_or(image_tag);

    let mut files = vec![
        PlannedFile {
            path: base.join("deployment.yaml"),
            content: generate::render(GenKind::Deployment, &workload)?,
        },
        PlannedFile {
            path: base.join("service.yaml"),
            content: generate::render(GenKind::Service, &service)?,
        },
    ];
    files.extend(manifests);
    files.push(PlannedFile {
        path: base.join("kustomization.yaml"),
        content: util::ensure_trailing_newline(base_kustomization),
    });

    for env in &args.envs {
        let overlay = KustomizationOverlayTpl {
            ctx: &KustomizeOverlayContext {
                name: args.name.clone(),
                namespace: args.namespace.clone(),
                image: image_name.clone(),
                new_tag: new_tag.clone(),
                replicas: args.replicas,
                env: env.clone(),
            },
        }
        .render()?;

        files.push(PlannedFile {
            path: root.join("overlays").join(env).join("kustomization.yaml"),
            content: util::ensure_trailing_newline(overlay),
        });
    }

    Ok(files)
}

/// Render the whole scaffold as text, used for `--dry-run` and the TUI preview.
pub fn render_tree(args: &ScaffoldArgs) -> Result<String> {
    validate(args)?;
    let files = plan(args)?;
    let root = args.dir.join(&args.name);

    let mut out = format!("{}\n", root.display());
    for file in &files {
        let relative = file.path.strip_prefix(&root).unwrap_or(&file.path);
        out.push_str(&format!("\n─── {} ───\n", relative.display()));
        out.push_str(&file.content);
    }

    Ok(out)
}

/// Write the scaffold to disk, returning the paths that were created.
pub fn write_tree(args: &ScaffoldArgs) -> Result<Vec<PathBuf>> {
    validate(args)?;
    let files = plan(args)?;
    let root = args.dir.join(&args.name);

    if root.exists() && !args.force {
        anyhow::bail!(
            "{} already exists (pass --force to overwrite)",
            root.display()
        );
    }

    let mut written = Vec::new();
    for file in &files {
        util::write_file(&file.path, &file.content, true)?;
        written.push(file.path.clone());
    }

    Ok(written)
}

fn validate(args: &ScaffoldArgs) -> Result<()> {
    util::validate_dns1123(&args.name, "application name")?;
    for env in &args.envs {
        util::validate_dns1123(env, "environment name")?;
    }
    Ok(())
}

pub fn run(args: ScaffoldArgs) -> Result<()> {
    if args.dry_run {
        print!("{}", render_tree(&args)?);
        return Ok(());
    }

    let root = args.dir.join(&args.name);
    for path in write_tree(&args)? {
        eprintln!("created {}", path.display());
    }

    eprintln!(
        "\nScaffold ready. Preview it with:\n  kubectl kustomize {}",
        root.display()
    );

    Ok(())
}
