use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::CtxArgs;
use crate::util::Palette;

#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub name: String,
    pub current: bool,
}

/// Run kubectl and return trimmed stdout.
fn kubectl(args: &[&str]) -> Result<String> {
    let output = Command::new("kubectl")
        .args(args)
        .output()
        .context("running kubectl — is it installed and on PATH?")?;

    if !output.status.success() {
        bail!(
            "kubectl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn current_context() -> Option<String> {
    kubectl(&["config", "current-context"])
        .ok()
        .filter(|name| !name.is_empty())
}

pub fn current_namespace() -> Option<String> {
    kubectl(&["config", "view", "--minify", "-o", "jsonpath={..namespace}"])
        .ok()
        .filter(|namespace| !namespace.is_empty())
}

pub fn list_contexts() -> Result<Vec<ContextEntry>> {
    let listed = kubectl(&["config", "get-contexts", "-o", "name"])?;
    let current = current_context();

    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|name| ContextEntry {
            name: name.to_string(),
            current: current.as_deref() == Some(name),
        })
        .collect())
}

pub fn use_context(name: &str) -> Result<()> {
    kubectl(&["config", "use-context", name])?;
    Ok(())
}

pub fn set_namespace(namespace: &str, context: Option<&str>) -> Result<()> {
    match context {
        Some(name) => kubectl(&["config", "set-context", name, "--namespace", namespace])?,
        None => kubectl(&[
            "config",
            "set-context",
            "--current",
            "--namespace",
            namespace,
        ])?,
    };
    Ok(())
}

pub fn run(args: CtxArgs) -> Result<()> {
    let palette = Palette::auto(false);

    if args.current {
        match current_context() {
            Some(name) => println!("{name}"),
            None => eprintln!("no current context — is a kubeconfig configured?"),
        }
        return Ok(());
    }

    if let Some(name) = &args.set {
        use_context(name)?;
        println!("switched to context {}", palette.green(name));
    }

    if let Some(namespace) = &args.namespace {
        set_namespace(namespace, args.set.as_deref())?;
        println!(
            "namespace set to {} on {}",
            palette.green(namespace),
            args.set.as_deref().unwrap_or("the current context")
        );
    }

    if args.set.is_none() && args.namespace.is_none() {
        let contexts = list_contexts()?;
        if contexts.is_empty() {
            eprintln!("no contexts found in your kubeconfig");
            return Ok(());
        }

        let namespace = current_namespace();
        for entry in contexts {
            if entry.current {
                println!("* {}", palette.green(&entry.name));
            } else {
                println!("  {}", entry.name);
            }
        }

        if let Some(namespace) = namespace {
            println!("\nnamespace: {}", palette.cyan(&namespace));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_kubectl_or_kubeconfig_does_not_panic() {
        // Either kubectl is absent or no kubeconfig is configured in CI; both
        // must degrade to `None` rather than unwinding.
        let _ = current_context();
        let _ = current_namespace();
    }
}
