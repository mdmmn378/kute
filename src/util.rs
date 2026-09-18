use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Parse a `KEY=VALUE` argument.
pub fn parse_kv(input: &str) -> Result<(String, String), String> {
    match input.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
        _ => Err(format!("expected KEY=VALUE, got `{input}`")),
    }
}

/// Parse a `NAME:/mount/path` argument into `(source, mount_path)`.
pub fn parse_mount(input: &str) -> Result<(String, String), String> {
    match input.split_once(':') {
        Some((name, path)) if !name.is_empty() && path.starts_with('/') => {
            Ok((name.to_string(), path.to_string()))
        }
        _ => Err(format!(
            "expected NAME:/mount/path (absolute path), got `{input}`"
        )),
    }
}

/// The pieces of a `VERBS:RESOURCES[:APIGROUPS]` rule before it is narrowed
/// into a concrete RBAC rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRule {
    pub verbs: Vec<String>,
    pub resources: Vec<String>,
    pub api_groups: Vec<String>,
}

/// Parse `verbs:resources[:apiGroups]`, e.g. `get,list:pods,services:apps`.
pub fn parse_rule(input: &str) -> Result<RawRule, String> {
    let mut parts = input.split(':');
    let verbs = split_list(parts.next().unwrap_or_default());
    let resources = split_list(parts.next().unwrap_or_default());
    let api_groups = match parts.next() {
        Some(groups) => split_list(groups),
        None => vec![String::new()],
    };

    if verbs.is_empty() || resources.is_empty() {
        return Err(format!(
            "expected VERBS:RESOURCES[:APIGROUPS], e.g. `get,list:pods`, got `{input}`"
        ));
    }

    Ok(RawRule {
        verbs,
        resources,
        api_groups,
    })
}

pub fn split_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn ensure_trailing_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Write `content` to `output`, or to stdout when `output` is `None`.
pub fn emit(content: &str, output: Option<&Path>, force: bool) -> Result<()> {
    match output {
        None => {
            print!("{content}");
            Ok(())
        }
        Some(path) => {
            write_file(path, content, force)?;
            eprintln!("wrote {}", path.display());
            Ok(())
        }
    }
}

/// Write a file, creating parent directories, refusing to clobber unless forced.
pub fn write_file(path: &Path, content: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists (pass --force to overwrite)",
            path.display()
        );
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Names must be valid DNS-1123 subdomain labels to be accepted by the API server.
pub fn validate_dns1123(name: &str, what: &str) -> Result<()> {
    if name.is_empty() || name.len() > 253 {
        bail!("{what} must be 1-253 characters, got `{name}`");
    }

    let valid = name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });

    if !valid {
        bail!(
            "{what} `{name}` must be a lowercase RFC 1123 name \
             (lowercase alphanumerics, `-` or `.`, starting and ending with an alphanumeric)"
        );
    }

    Ok(())
}

/// Minimal ANSI styling that degrades to plain text when colour is disabled.
#[derive(Copy, Clone, Debug)]
pub struct Palette {
    pub enabled: bool,
}

impl Palette {
    /// Colour only when stdout is a terminal, unless the user opted out.
    pub fn auto(no_color: bool) -> Self {
        use std::io::IsTerminal;
        Self {
            enabled: !no_color && std::io::stdout().is_terminal(),
        }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }

    pub fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }

    pub fn cyan(&self, text: &str) -> String {
        self.wrap("36", text)
    }

    pub fn green(&self, text: &str) -> String {
        self.wrap("32", text)
    }
}
