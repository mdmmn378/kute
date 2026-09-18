use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::cli::{SearchArgs, SearchSource};
use crate::history;
use crate::snippets;
use crate::util::Palette;

/// Where a candidate came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Example,
    History,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Example => "examples",
            Origin::History => "history",
        }
    }

    /// Examples outrank history when scores are equal: they are curated.
    fn rank(self) -> u8 {
        match self {
            Origin::Example => 0,
            Origin::History => 1,
        }
    }
}

/// A searchable command.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub cmd: String,
    pub desc: String,
    pub tags: Vec<String>,
    pub category: String,
    pub origin: Origin,
    pub uses: usize,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub index: usize,
    pub score: i64,
    /// Character indices of the matched characters inside `Candidate::cmd`.
    pub positions: Vec<usize>,
}

/// Gather candidates from the embedded corpus and/or shell history.
pub fn build_candidates(source: SearchSource, extra_history: &[PathBuf]) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    if matches!(source, SearchSource::All | SearchSource::Examples) {
        candidates.extend(snippets::load().into_iter().map(|snippet| Candidate {
            cmd: snippet.cmd,
            desc: snippet.desc,
            tags: snippet.tags,
            category: snippet.category,
            origin: Origin::Example,
            uses: 0,
        }));
    }

    if matches!(source, SearchSource::All | SearchSource::History) {
        let mut paths = history::default_history_files();
        paths.extend_from_slice(extra_history);

        let mut seen = HashSet::new();
        paths.retain(|path| seen.insert(path.clone()));

        candidates.extend(
            history::load_commands(&paths)
                .into_iter()
                .map(|(cmd, uses)| Candidate {
                    cmd,
                    desc: String::new(),
                    tags: Vec::new(),
                    category: "history".to_string(),
                    origin: Origin::History,
                    uses,
                }),
        );
    }

    candidates
}

/// Rank candidates against `query`. An empty query returns everything, with
/// curated examples first and then history ordered by how often it was used.
pub fn search(candidates: &[Candidate], query: &str, limit: usize) -> Vec<Hit> {
    let matcher = SkimMatcherV2::default();
    let query = query.trim();

    if query.is_empty() {
        let mut hits: Vec<Hit> = (0..candidates.len())
            .map(|index| Hit {
                index,
                score: 0,
                positions: Vec::new(),
            })
            .collect();

        hits.sort_by(|a, b| {
            let left = &candidates[a.index];
            let right = &candidates[b.index];
            left.origin
                .rank()
                .cmp(&right.origin.rank())
                .then_with(|| right.uses.cmp(&left.uses))
                .then_with(|| left.cmd.cmp(&right.cmd))
        });

        hits.truncate(limit);
        return hits;
    }

    // Each whitespace-separated token must match somewhere; tokens may match
    // different fields and in any order, so "service port-forward" and
    // "port-forward service" both find the same command.
    let tokens: Vec<&str> = query.split_whitespace().collect();

    let mut hits: Vec<Hit> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let mut total = 0;
            let mut positions = Vec::new();

            for token in &tokens {
                let command_match = matcher.fuzzy_indices(&candidate.cmd, token);
                let desc_score = matcher.fuzzy_match(&candidate.desc, token);
                let tag_score = candidate
                    .tags
                    .iter()
                    .filter_map(|tag| matcher.fuzzy_match(tag, token))
                    .max();

                // A command match always beats a description or tag match, so the
                // weaker signals are scaled down rather than compared directly.
                // If any token fails to match, the whole candidate is rejected.
                let best = command_match
                    .as_ref()
                    .map(|(score, _)| *score)
                    .into_iter()
                    .chain(desc_score.map(|score| score * 7 / 10))
                    .chain(tag_score.map(|score| score / 2))
                    .max()?;

                total += best;

                if let Some((_, indices)) = command_match {
                    positions.extend(indices);
                }
            }

            // Reward a candidate containing the whole query as one phrase.
            if let Some((phrase_score, indices)) = matcher.fuzzy_indices(&candidate.cmd, query) {
                total += phrase_score;
                positions = indices;
            }

            // Nudge frequently used history entries above equally-scored ones.
            let frequency_bonus = if candidate.origin == Origin::History {
                candidate.uses.min(5) as i64
            } else {
                0
            };

            Some(Hit {
                index,
                score: total + frequency_bonus,
                positions,
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                candidates[a.index]
                    .origin
                    .rank()
                    .cmp(&candidates[b.index].origin.rank())
            })
            .then_with(|| candidates[a.index].cmd.cmp(&candidates[b.index].cmd))
    });

    hits.truncate(limit);
    hits
}

/// Render a command, optionally truncating, with matched characters highlighted.
pub fn render_command(
    cmd: &str,
    positions: &[usize],
    max_width: Option<usize>,
    palette: &Palette,
) -> String {
    let chars: Vec<char> = cmd.chars().collect();

    let (visible_len, suffix) = match max_width {
        Some(max) if chars.len() > max => (max.saturating_sub(1), "…"),
        _ => (chars.len(), ""),
    };

    let matched: HashSet<usize> = positions.iter().copied().collect();
    let mut painted = String::new();

    for (index, character) in chars.iter().take(visible_len).enumerate() {
        if matched.contains(&index) {
            painted.push_str(&palette.bold(&palette.cyan(&character.to_string())));
        } else {
            painted.push(*character);
        }
    }

    painted.push_str(suffix);
    painted
}

#[derive(serde::Serialize)]
struct JsonHit<'a> {
    command: &'a str,
    description: &'a str,
    category: &'a str,
    source: &'a str,
    score: i64,
    uses: usize,
}

pub fn run(args: SearchArgs) -> Result<()> {
    if args.interactive {
        return crate::tui::run_search(&args);
    }

    let query = args.query.join(" ");
    let candidates = build_candidates(args.source, &args.history_file);
    let hits = search(&candidates, &query, args.limit);
    let palette = Palette::auto(args.no_color);

    if args.print {
        if let Some(hit) = hits.first() {
            println!("{}", candidates[hit.index].cmd);
        }
        return Ok(());
    }

    if args.json {
        let payload: Vec<JsonHit<'_>> = hits
            .iter()
            .map(|hit| {
                let candidate = &candidates[hit.index];
                JsonHit {
                    command: &candidate.cmd,
                    description: &candidate.desc,
                    category: &candidate.category,
                    source: candidate.origin.label(),
                    score: hit.score,
                    uses: candidate.uses,
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if hits.is_empty() {
        eprintln!("no matches for `{query}`");
        return Ok(());
    }

    let max_width = if args.full { None } else { Some(78) };

    for (rank, hit) in hits.iter().enumerate() {
        let candidate = &candidates[hit.index];
        let command = render_command(&candidate.cmd, &hit.positions, max_width, &palette);

        let mut suffix = String::new();
        if !candidate.desc.is_empty() {
            suffix.push_str("  — ");
            suffix.push_str(&candidate.desc);
        }
        suffix.push_str(&format!("  [{}", candidate.origin.label()));
        if candidate.uses > 1 {
            suffix.push_str(&format!(" ×{}", candidate.uses));
        }
        suffix.push(']');

        println!("{:>3}. {}{}", rank + 1, command, palette.dim(&suffix));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(cmd: &str, desc: &str, origin: Origin) -> Candidate {
        Candidate {
            cmd: cmd.to_string(),
            desc: desc.to_string(),
            tags: Vec::new(),
            category: "test".to_string(),
            origin,
            uses: 0,
        }
    }

    fn sample() -> Vec<Candidate> {
        vec![
            candidate(
                "kubectl get pods -A -o wide",
                "List every pod",
                Origin::Example,
            ),
            candidate(
                "kubectl apply -f manifest.yaml",
                "Apply a manifest",
                Origin::Example,
            ),
            candidate("kubectl logs -f app", "Tail logs", Origin::History),
        ]
    }

    #[test]
    fn finds_the_obvious_command() {
        let candidates = sample();
        let hits = search(&candidates, "get pods", 10);
        assert_eq!(candidates[hits[0].index].cmd, "kubectl get pods -A -o wide");
    }

    #[test]
    fn tolerates_typos_and_gaps() {
        let candidates = sample();
        let hits = search(&candidates, "kubectl lgs", 10);
        assert_eq!(candidates[hits[0].index].cmd, "kubectl logs -f app");
    }

    #[test]
    fn highlights_matched_characters() {
        let candidates = sample();
        let hits = search(&candidates, "apply", 10);
        assert!(!hits[0].positions.is_empty());
    }

    #[test]
    fn empty_query_lists_examples_first() {
        let candidates = sample();
        let hits = search(&candidates, "", 10);
        assert_eq!(hits.len(), 3);
        assert_eq!(candidates[hits[0].index].origin, Origin::Example);
        assert_eq!(candidates[hits[2].index].origin, Origin::History);
    }

    #[test]
    fn respects_the_limit() {
        let candidates = sample();
        assert_eq!(search(&candidates, "", 2).len(), 2);
    }

    #[test]
    fn returns_nothing_for_nonsense() {
        let candidates = sample();
        assert!(search(&candidates, "zzzzqqqqxxxx", 10).is_empty());
    }

    #[test]
    fn command_matches_beat_description_matches() {
        let candidates = vec![
            candidate(
                "kubectl describe node",
                "Show pod scheduling",
                Origin::Example,
            ),
            candidate("kubectl get pods", "List pods", Origin::Example),
        ];
        let hits = search(&candidates, "pods", 10);
        assert_eq!(candidates[hits[0].index].cmd, "kubectl get pods");
    }

    #[test]
    fn tokens_match_in_any_order_and_across_fields() {
        let candidates = vec![
            candidate(
                "kubectl port-forward -n NS svc/NAME 8080:80",
                "Tunnel a service to localhost",
                Origin::Example,
            ),
            candidate("kubectl get pods -A", "List every pod", Origin::Example),
        ];

        // "service" only appears in the description here, "port-forward" only in
        // the command; the candidate should match regardless of token order.
        for query in ["port-forward service", "service port-forward"] {
            let hits = search(&candidates, query, 10);
            assert_eq!(
                candidates[hits[0].index].cmd, "kubectl port-forward -n NS svc/NAME 8080:80",
                "failed for query `{query}`"
            );
        }
    }

    #[test]
    fn every_token_must_match() {
        let candidates = sample();
        assert!(search(&candidates, "apply zzzzqqqq", 10).is_empty());
    }
}
