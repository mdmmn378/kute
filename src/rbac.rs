use std::ops::Deref;

use anyhow::{Result, bail};
use askama::Template;

use crate::cli::{
    RbacArgs, RbacBindingArgs, RbacBundleArgs, RbacCommand, RbacRoleArgs, RbacServiceAccountArgs,
};
use crate::generate;
use crate::model::{BindingContext, GenContext, Label, RbacContext, Subject};
use crate::util;

#[derive(Template)]
#[template(path = "rbac_role.yaml")]
struct RoleTpl<'a> {
    ctx: &'a RbacContext,
}

impl<'a> Deref for RoleTpl<'a> {
    type Target = RbacContext;

    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

#[derive(Template)]
#[template(path = "rbac_binding.yaml")]
struct BindingTpl<'a> {
    ctx: &'a BindingContext,
}

impl<'a> Deref for BindingTpl<'a> {
    type Target = BindingContext;

    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

fn rbac_labels(name: &str) -> Vec<Label> {
    vec![
        Label::new("app.kubernetes.io/name", name),
        Label::new("app.kubernetes.io/managed-by", "kute"),
    ]
}

pub fn render_role(kind: &str, args: &RbacRoleArgs) -> Result<String> {
    if args.rules.is_empty() {
        bail!(
            "at least one --rule is required, e.g. \
             --rule get,list,watch:pods,services"
        );
    }
    util::validate_dns1123(&args.name, "role name")?;

    let mut rules = args.rules.clone();
    for rule in &mut rules {
        rule.resource_names = args.resource_names.clone();
    }

    // A ClusterRole is cluster-scoped, so it must not carry a namespace.
    let namespace = if kind == "ClusterRole" {
        None
    } else {
        args.namespace.clone()
    };

    let ctx = RbacContext {
        kind: kind.to_string(),
        name: args.name.clone(),
        namespace,
        labels: rbac_labels(&args.name),
        annotations: Vec::new(),
        rules,
    };

    let rendered = RoleTpl { ctx: &ctx }.render()?;
    Ok(util::ensure_trailing_newline(rendered))
}

pub fn render_binding(kind: &str, args: &RbacBindingArgs) -> Result<String> {
    let role_kind = if kind == "ClusterRoleBinding" {
        "ClusterRole"
    } else {
        "Role"
    };
    util::validate_dns1123(&args.name, "binding name")?;
    util::validate_dns1123(&args.role, "role reference")?;

    if args.service_accounts.is_empty() {
        bail!("at least one --service-account is required");
    }

    let subjects = args
        .service_accounts
        .iter()
        .map(|sa| Subject {
            kind: "ServiceAccount".to_string(),
            name: sa.name.clone(),
            namespace: Some(
                sa.namespace
                    .clone()
                    .or_else(|| args.namespace.clone())
                    .unwrap_or_else(|| "default".to_string()),
            ),
        })
        .collect();

    let namespace = if kind == "ClusterRoleBinding" {
        None
    } else {
        args.namespace.clone()
    };

    let ctx = BindingContext {
        kind: kind.to_string(),
        name: args.name.clone(),
        namespace,
        labels: rbac_labels(&args.name),
        annotations: Vec::new(),
        role_ref_kind: role_kind.to_string(),
        role_ref_name: args.role.clone(),
        subjects,
    };

    let rendered = BindingTpl { ctx: &ctx }.render()?;
    Ok(util::ensure_trailing_newline(rendered))
}

pub fn render_service_account(args: &RbacServiceAccountArgs) -> Result<String> {
    util::validate_dns1123(&args.name, "service account name")?;

    let mut ctx = GenContext::new(&args.name);
    ctx.namespace = args.namespace.clone();
    ctx.automount = !args.no_automount;
    ctx.image_pull_secrets = args.image_pull_secrets.clone();

    generate::render(crate::model::GenKind::ServiceAccount, &ctx)
}

/// ServiceAccount + Role (or ClusterRole) + binding, as a multi-document file.
pub fn render_bundle(args: &RbacBundleArgs) -> Result<String> {
    if args.rules.is_empty() {
        bail!(
            "at least one --rule is required, e.g. \
             --rule get,list,watch:pods,services"
        );
    }
    util::validate_dns1123(&args.name, "bundle name")?;

    let role_kind = if args.cluster_wide {
        "ClusterRole"
    } else {
        "Role"
    };
    let binding_kind = if args.cluster_wide {
        "ClusterRoleBinding"
    } else {
        "RoleBinding"
    };

    let sa_args = RbacServiceAccountArgs {
        name: args.name.clone(),
        namespace: Some(args.namespace.clone()),
        no_automount: args.no_automount,
        image_pull_secrets: args.image_pull_secrets.clone(),
        output: None,
        force: false,
    };
    let service_account = render_service_account(&sa_args)?;

    let role_args = RbacRoleArgs {
        name: args.name.clone(),
        namespace: Some(args.namespace.clone()),
        rules: args.rules.clone(),
        resource_names: args.resource_names.clone(),
        output: None,
        force: false,
    };
    let role = render_role(role_kind, &role_args)?;

    let binding_args = RbacBindingArgs {
        name: args.name.clone(),
        namespace: Some(args.namespace.clone()),
        role: args.name.clone(),
        service_accounts: vec![crate::cli::SubjectArg {
            namespace: Some(args.namespace.clone()),
            name: args.name.clone(),
        }],
        output: None,
        force: false,
    };
    let binding = render_binding(binding_kind, &binding_args)?;

    Ok(format!("{service_account}---\n{role}---\n{binding}"))
}

pub fn run(args: RbacArgs) -> Result<()> {
    match args.command {
        RbacCommand::ServiceAccount(sa) => {
            let manifest = render_service_account(&sa)?;
            util::emit(&manifest, sa.output.as_deref(), sa.force)
        }
        RbacCommand::Role(role) => {
            let manifest = render_role("Role", &role)?;
            util::emit(&manifest, role.output.as_deref(), role.force)
        }
        RbacCommand::ClusterRole(role) => {
            let manifest = render_role("ClusterRole", &role)?;
            util::emit(&manifest, role.output.as_deref(), role.force)
        }
        RbacCommand::RoleBinding(binding) => {
            let manifest = render_binding("RoleBinding", &binding)?;
            util::emit(&manifest, binding.output.as_deref(), binding.force)
        }
        RbacCommand::ClusterRoleBinding(binding) => {
            let manifest = render_binding("ClusterRoleBinding", &binding)?;
            util::emit(&manifest, binding.output.as_deref(), binding.force)
        }
        RbacCommand::Bundle(bundle) => {
            let manifest = render_bundle(&bundle)?;
            util::emit(&manifest, bundle.output.as_deref(), bundle.force)
        }
    }
}
