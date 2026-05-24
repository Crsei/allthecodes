use std::collections::HashMap;

use super::super::flag_validation::map_from_pairs;
use super::super::types::{ExternalCommandConfig, FlagArgType};

pub(crate) fn make_gh_read_only_commands() -> HashMap<String, ExternalCommandConfig> {
    let mut m = HashMap::new();

    // gh (root) — just `gh` with no subcommand is read-only (help)
    // Additional check blocks subcommand-like positional args so `gh pr merge` etc.
    // don't accidentally match the root entry.
    m.insert(
        "gh".into(),
        ExternalCommandConfig::with_check(map_from_pairs(&[]), |_raw: &str, args: &[String]| {
            // If there are non-flag positional args after bare "gh",
            // this is a subcommand, not just help — block it.
            args.iter().any(|a| !a.starts_with('-'))
        }),
    );

    // gh pr view
    m.insert(
        "gh pr view".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
            ("--comments", FlagArgType::None),
            ("-c", FlagArgType::None),
            ("--web", FlagArgType::None),
            ("-w", FlagArgType::None),
        ])),
    );

    // gh pr list
    m.insert(
        "gh pr list".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-a", FlagArgType::String),
            ("--assignee", FlagArgType::String),
            ("-l", FlagArgType::String),
            ("--label", FlagArgType::String),
            ("-L", FlagArgType::Number),
            ("--limit", FlagArgType::Number),
            ("-s", FlagArgType::String),
            ("--state", FlagArgType::String),
            ("--author", FlagArgType::String),
            ("--base", FlagArgType::String),
            ("--draft", FlagArgType::None),
            ("--search", FlagArgType::String),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("--web", FlagArgType::None),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
        ])),
    );

    // gh issue list
    m.insert(
        "gh issue list".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-a", FlagArgType::String),
            ("--assignee", FlagArgType::String),
            ("-l", FlagArgType::String),
            ("--label", FlagArgType::String),
            ("-L", FlagArgType::Number),
            ("--limit", FlagArgType::Number),
            ("-s", FlagArgType::String),
            ("--state", FlagArgType::String),
            ("--author", FlagArgType::String),
            ("--search", FlagArgType::String),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("--web", FlagArgType::None),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
        ])),
    );

    // gh issue view
    m.insert(
        "gh issue view".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
            ("--comments", FlagArgType::None),
            ("-c", FlagArgType::None),
            ("--web", FlagArgType::None),
            ("-w", FlagArgType::None),
        ])),
    );

    // gh run list
    m.insert(
        "gh run list".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-L", FlagArgType::Number),
            ("--limit", FlagArgType::Number),
            ("-w", FlagArgType::String),
            ("--workflow", FlagArgType::String),
            ("-b", FlagArgType::String),
            ("--branch", FlagArgType::String),
            ("-u", FlagArgType::String),
            ("--user", FlagArgType::String),
            ("-c", FlagArgType::String),
            ("--commit", FlagArgType::String),
            ("-e", FlagArgType::String),
            ("--event", FlagArgType::String),
            ("--status", FlagArgType::String),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
        ])),
    );

    // gh run view
    m.insert(
        "gh run view".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
            ("--exit-status", FlagArgType::None),
            ("--log", FlagArgType::None),
            ("--log-failed", FlagArgType::None),
            ("-v", FlagArgType::None),
            ("--verbose", FlagArgType::None),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("--attempt", FlagArgType::Number),
        ])),
    );

    // gh search prs
    m.insert(
        "gh search prs".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
        ])),
    );

    // gh search issues
    m.insert(
        "gh search issues".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--json", FlagArgType::String),
            ("-q", FlagArgType::String),
            ("--jq", FlagArgType::String),
            ("--template", FlagArgType::String),
            ("-R", FlagArgType::String),
            ("--repo", FlagArgType::String),
        ])),
    );

    // gh api (GET only)
    m.insert(
        "gh api".into(),
        ExternalCommandConfig::with_check(
            map_from_pairs(&[
                ("-H", FlagArgType::String),
                ("--header", FlagArgType::String),
                ("-F", FlagArgType::String),
                ("--field", FlagArgType::String),
                ("-q", FlagArgType::String),
                ("--jq", FlagArgType::String),
                ("--template", FlagArgType::String),
                ("--paginate", FlagArgType::None),
                ("--include", FlagArgType::None),
                ("--cache", FlagArgType::Number),
                ("--silent", FlagArgType::None),
            ]),
            |_raw: &str, args: &[String]| {
                // Block if first positional is not a GET method
                // `gh api` without method defaults to GET
                for token in args {
                    if token.is_empty() {
                        continue;
                    }
                    if token.starts_with('-') {
                        continue;
                    }
                    // First positional: could be a URL or method
                    // Block POST, PUT, PATCH, DELETE
                    let upper = token.to_uppercase();
                    if upper == "POST" || upper == "PUT" || upper == "PATCH" || upper == "DELETE" {
                        return true;
                    }
                    return false; // URL or GET method is safe
                }
                false
            },
        ),
    );

    m
}
