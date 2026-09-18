use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Shell aliases that unambiguously mean "kubectl", plus single letters that
/// need a following verb to be convincing.
const ALIASES: &[&str] = &[
    "kubectl", "kgp", "kgs", "kgn", "kgd", "kga", "kg", "kdp", "kds", "kdd", "kdel", "kl", "klo",
    "kd", "ke", "kpf", "kex", "ked", "kaf", "ktp",
];

const AMBIGUOUS: &[&str] = &["k", "kc"];

const VERBS: &[&str] = &[
    "get",
    "describe",
    "apply",
    "delete",
    "logs",
    "log",
    "exec",
    "create",
    "edit",
    "scale",
    "rollout",
    "port-forward",
    "top",
    "explain",
    "config",
    "auth",
    "drain",
    "cordon",
    "uncordon",
    "patch",
    "replace",
    "cp",
    "diff",
    "wait",
    "label",
    "annotate",
    "set",
    "debug",
    "api-resources",
    "version",
    "cluster-info",
    "expose",
    "run",
];

/// History files we probe by default, in no particular order.
pub fn default_history_files() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(histfile) = std::env::var_os("HISTFILE") {
        paths.push(PathBuf::from(histfile));
    }

    if let Some(home) = dirs::home_dir() {
        for relative in [
            ".zsh_history",
            ".bash_history",
            ".histfile",
            ".local/share/fish/fish_history",
        ] {
            paths.push(home.join(relative));
        }
    }

    paths
}

/// Strip shell-specific history prefixes.
fn normalize(line: &str) -> String {
    let line = line.trim();

    // zsh extended history: `: 1712345678:0;kubectl get pods`
    if let Some(rest) = line.strip_prefix(": ")
        && let Some((_, command)) = rest.split_once(';')
    {
        return command.trim().to_string();
    }

    // fish: `- cmd: kubectl get pods`
    if let Some(rest) = line.strip_prefix("- cmd: ") {
        return rest.trim().to_string();
    }

    line.to_string()
}

/// Decide whether a history line is worth offering as a kubectl command.
pub fn is_kubectl_like(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(first) = parts.next() else {
        return false;
    };

    if ALIASES.contains(&first) {
        return true;
    }

    if AMBIGUOUS.contains(&first) {
        return matches!(parts.next(), Some(verb) if VERBS.contains(&verb));
    }

    false
}

/// Extract every kubectl-ish command from one history file.
pub fn parse_history(path: &Path) -> Vec<String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    let mut buffer = String::new();

    for raw in contents.lines() {
        let line = normalize(raw);

        if line.is_empty() && buffer.is_empty() {
            continue;
        }

        if !buffer.is_empty() {
            buffer.push(' ');
        }

        let continued = line.ends_with('\\');
        buffer.push_str(line.trim_end_matches('\\').trim_end());

        if continued {
            continue;
        }

        let command = std::mem::take(&mut buffer);
        if is_kubectl_like(&command) {
            commands.push(command);
        }
    }

    if !buffer.is_empty() && is_kubectl_like(&buffer) {
        commands.push(buffer);
    }

    commands
}

/// Every kubectl command across the given files, with usage counts, most used first.
pub fn load_commands(paths: &[PathBuf]) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for path in paths {
        for command in parse_history(path) {
            *counts.entry(command).or_insert(0) += 1;
        }
    }

    let mut commands: Vec<(String, usize)> = counts.into_iter().collect();
    commands.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_history(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kute-history-test-{name}"));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn recognizes_kubectl_invocations() {
        assert!(is_kubectl_like("kubectl get pods"));
        assert!(is_kubectl_like("k get pods"));
        assert!(is_kubectl_like("kc get pods"));
        assert!(is_kubectl_like("kgp -A"));
        assert!(is_kubectl_like("kl app-123"));
    }

    #[test]
    fn ignores_unrelated_and_ambiguous_lines() {
        assert!(!is_kubectl_like("k something"));
        assert!(!is_kubectl_like("kc something"));
        assert!(!is_kubectl_like("git status"));
        assert!(!is_kubectl_like(""));
        assert!(!is_kubectl_like("k9s"));
    }

    #[test]
    fn parses_zsh_extended_format() {
        let path = temp_history(
            "zsh",
            ": 1712345678:0;kubectl get pods -A\n: 1712345679:0;ls -la\n",
        );
        assert_eq!(parse_history(&path), vec!["kubectl get pods -A"]);
    }

    #[test]
    fn parses_fish_format() {
        let path = temp_history("fish", "- cmd: kubectl get ns\n- cmd: vim foo\n");
        assert_eq!(parse_history(&path), vec!["kubectl get ns"]);
    }

    #[test]
    fn joins_continued_lines() {
        let path = temp_history(
            "cont",
            "kubectl get pods \\\n  -n kube-system \\\n  -o wide\n",
        );
        assert_eq!(
            parse_history(&path),
            vec!["kubectl get pods -n kube-system -o wide"]
        );
    }

    #[test]
    fn counts_duplicates() {
        let path = temp_history(
            "counts",
            "kubectl get pods\nkubectl get pods\nkubectl get svc\n",
        );
        let commands = load_commands(&[path]);
        assert_eq!(commands[0], ("kubectl get pods".to_string(), 2));
        assert_eq!(commands[1], ("kubectl get svc".to_string(), 1));
    }

    #[test]
    fn missing_file_is_not_an_error() {
        assert!(parse_history(Path::new("/nonexistent/kute/history")).is_empty());
    }
}
