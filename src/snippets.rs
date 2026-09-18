use serde::Deserialize;

/// One kubectl example from the embedded corpus.
#[derive(Debug, Clone, Deserialize)]
pub struct Snippet {
    pub cmd: String,
    pub desc: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: String,
}

#[derive(Deserialize)]
struct Corpus {
    #[serde(default)]
    snippet: Vec<Snippet>,
}

/// The corpus is compiled into the binary, so shipping a new `data/kubectl.toml`
/// is all it takes to extend the built-in examples.
const CORPUS: &str = include_str!("../data/kubectl.toml");

pub fn load() -> Vec<Snippet> {
    let corpus: Corpus =
        toml::from_str(CORPUS).expect("embedded corpus data/kubectl.toml must be valid TOML");
    corpus.snippet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_parses_and_is_populated() {
        let snippets = load();
        assert!(
            snippets.len() > 50,
            "expected a substantial corpus, got {}",
            snippets.len()
        );
    }

    #[test]
    fn corpus_entries_are_well_formed() {
        for snippet in load() {
            assert!(
                snippet.cmd.starts_with("kubectl"),
                "`{}` should start with kubectl",
                snippet.cmd
            );
            assert!(
                !snippet.desc.trim().is_empty(),
                "`{}` needs a desc",
                snippet.cmd
            );
            assert!(
                !snippet.category.trim().is_empty(),
                "`{}` needs a category",
                snippet.cmd
            );
        }
    }

    #[test]
    fn corpus_has_no_placeholder_text() {
        for snippet in load() {
            assert!(
                !snippet.cmd.contains(" and "),
                "`{}` looks like prose, not a command",
                snippet.cmd
            );
            assert!(
                !snippet.cmd.contains("\\\""),
                "`{}` has a stray backslash-quote escape",
                snippet.cmd
            );
        }
    }
}
