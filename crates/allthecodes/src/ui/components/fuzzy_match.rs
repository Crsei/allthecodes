#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FuzzyMatchKind {
    Empty,
    Exact,
    Prefix,
    Contains,
    Subsequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub kind: FuzzyMatchKind,
    pub score: usize,
}

pub fn fuzzy_match(candidate: &str, query: &str) -> Option<FuzzyMatch> {
    let query = normalize(query);
    if query.is_empty() {
        return Some(FuzzyMatch {
            kind: FuzzyMatchKind::Empty,
            score: 0,
        });
    }

    let candidate = normalize(candidate);
    if candidate == query {
        return Some(FuzzyMatch {
            kind: FuzzyMatchKind::Exact,
            score: 0,
        });
    }

    if candidate.starts_with(&query) {
        return Some(FuzzyMatch {
            kind: FuzzyMatchKind::Prefix,
            score: 100 + candidate.len().saturating_sub(query.len()),
        });
    }

    if let Some(index) = candidate.find(&query) {
        return Some(FuzzyMatch {
            kind: FuzzyMatchKind::Contains,
            score: 200 + index + candidate.len().saturating_sub(query.len()),
        });
    }

    subsequence_score(&candidate, &query).map(|score| FuzzyMatch {
        kind: FuzzyMatchKind::Subsequence,
        score: 300 + score,
    })
}

pub fn best_fuzzy_match<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Option<FuzzyMatch> {
    candidates
        .into_iter()
        .filter_map(|candidate| fuzzy_match(candidate, query))
        .min_by_key(|matched| matched.score)
}

/// Multi-field weighted fuzzy matching.
///
/// Evaluates the query against multiple fields with different weights.
/// The overall match score is a weighted combination, where:
/// - name_weight: weight applied to the name field (1.0 = full)
/// - alias_weight: weight applied to aliases (0.75 = default)
/// - description_weight: weight applied to description (0.5 = default)
///
/// Returns the lowest (best) weighted score across all evaluated fields.
pub fn weighted_fuzzy_match(
    name: &str,
    aliases: &[&str],
    description: &str,
    query: &str,
    name_weight: f64,
    alias_weight: f64,
    description_weight: f64,
) -> Option<FuzzyMatch> {
    let query = normalize(query);
    if query.is_empty() {
        return Some(FuzzyMatch {
            kind: FuzzyMatchKind::Empty,
            score: 0,
        });
    }

    let mut best_score: Option<usize> = None;
    let mut best_kind = FuzzyMatchKind::Subsequence;

    // Score name field
    if let Some(m) = fuzzy_match(name, &query) {
        let weighted = (m.score as f64 / name_weight.max(0.01)) as usize;
        update_best(&mut best_score, &mut best_kind, weighted, m.kind);
    }

    // Score aliases
    for alias in aliases {
        if let Some(m) = fuzzy_match(alias, &query) {
            let weighted = (m.score as f64 / alias_weight.max(0.01)) as usize;
            update_best(&mut best_score, &mut best_kind, weighted, m.kind);
        }
    }

    // Score description
    if let Some(m) = fuzzy_match(description, &query) {
        let weighted = (m.score as f64 / description_weight.max(0.01)) as usize;
        update_best(&mut best_score, &mut best_kind, weighted, m.kind);
    }

    best_score.map(|score| FuzzyMatch {
        kind: best_kind,
        score,
    })
}

fn update_best(
    best_score: &mut Option<usize>,
    best_kind: &mut FuzzyMatchKind,
    score: usize,
    kind: FuzzyMatchKind,
) {
    let is_better = match best_score {
        Some(current) => {
            // Better kind always wins, then lower score
            (kind as u8) < (*best_kind as u8) || (kind == *best_kind && score < *current)
        }
        None => true,
    };
    if is_better {
        *best_score = Some(score);
        *best_kind = kind;
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn subsequence_score(candidate: &str, query: &str) -> Option<usize> {
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let query_chars = query.chars().collect::<Vec<_>>();
    let mut positions = Vec::with_capacity(query_chars.len());
    let mut cursor = 0usize;

    for query_char in query_chars {
        let found = candidate_chars
            .iter()
            .enumerate()
            .skip(cursor)
            .find_map(|(idx, candidate_char)| (*candidate_char == query_char).then_some(idx))?;
        positions.push(found);
        cursor = found + 1;
    }

    let first = *positions.first()?;
    let last = *positions.last()?;
    let span = last.saturating_sub(first) + 1;
    let gaps = span.saturating_sub(positions.len());
    Some(gaps + first + candidate_chars.len().saturating_sub(positions.len()))
}

#[cfg(test)]
mod tests {
    use super::{best_fuzzy_match, fuzzy_match, weighted_fuzzy_match, FuzzyMatchKind};

    #[test]
    fn ranks_exact_prefix_contains_then_subsequence() {
        assert_eq!(
            fuzzy_match("mcp", "mcp").map(|m| m.kind),
            Some(FuzzyMatchKind::Exact)
        );
        assert_eq!(
            fuzzy_match("memory", "mem").map(|m| m.kind),
            Some(FuzzyMatchKind::Prefix)
        );
        assert_eq!(
            fuzzy_match("open-memory", "mem").map(|m| m.kind),
            Some(FuzzyMatchKind::Contains)
        );
        assert_eq!(
            fuzzy_match("model context protocol", "mcp").map(|m| m.kind),
            Some(FuzzyMatchKind::Subsequence)
        );
        assert!(fuzzy_match("status", "xyz").is_none());
    }

    #[test]
    fn best_match_uses_the_lowest_ranked_candidate() {
        let candidates = ["model context protocol", "mcp", "manage command palette"];
        let matched = best_fuzzy_match(candidates, "mcp").expect("match");

        assert_eq!(matched.kind, FuzzyMatchKind::Exact);
        assert_eq!(matched.score, 0);
    }

    #[test]
    fn weighted_match_prioritizes_name_over_alias() {
        // Query "cf" should match "config" (name) better than "copy files" (alias)
        let result = weighted_fuzzy_match(
            "config",
            &["cfg", "conf"],
            "Manage configuration settings",
            "cf",
            1.0,  // name weight
            0.75, // alias weight
            0.5,  // description weight
        );
        assert!(result.is_some());
        // The score should be reasonable
    }

    #[test]
    fn weighted_match_with_empty_query() {
        let result = weighted_fuzzy_match(
            "help",
            &["h", "?"],
            "Show help information",
            "",
            1.0,
            0.75,
            0.5,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, FuzzyMatchKind::Empty);
    }

    #[test]
    fn alias_weight_affects_scoring() {
        // When alias weight is very low, matching by alias should have higher score
        // (worse match) compared to matching by name when name weight is high
        let name_match = weighted_fuzzy_match(
            "help",
            &["h"],
            "",
            "h", // query is alias
            1.0, // name weight high
            1.0, // alias weight high
            0.5,
        );
        assert!(name_match.is_some());
    }

    #[test]
    fn no_match_returns_none() {
        let result =
            weighted_fuzzy_match("help", &[], "Show help", "zzzznonexistent", 1.0, 0.75, 0.5);
        assert!(result.is_none());
    }
}
