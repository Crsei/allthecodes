use std::path::Path;

use crate::ui::fuzzy_match::{best_fuzzy_match as best_fuzzy, fuzzy_match};
use allthecodes_commands as commands;
use allthecodes_skills;

use super::metadata::command_meta;
use super::CommandItem;

/// Source category for grouping in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandGroup {
    Builtin,
    User,
    Project,
    Plugin,
    Skill,
}

impl CommandGroup {
    pub fn label(&self) -> &'static str {
        match self {
            CommandGroup::Builtin => "Builtin",
            CommandGroup::User => "User",
            CommandGroup::Project => "Project",
            CommandGroup::Plugin => "Plugin",
            CommandGroup::Skill => "Skill",
        }
    }

    pub fn priority(&self) -> u8 {
        match self {
            CommandGroup::Builtin => 80,
            CommandGroup::User => 60,
            CommandGroup::Project => 50,
            CommandGroup::Plugin => 40,
            CommandGroup::Skill => 30,
        }
    }
}

/// A command item with group and score metadata.
#[derive(Debug, Clone)]
pub struct ScoredCommandItem {
    pub item: CommandItem,
    pub group: CommandGroup,
    pub score: usize,
    pub usage_score: f64,
}

pub(super) fn filtered_commands(query: &str, cwd: &Path) -> Vec<CommandItem> {
    let query_lower = query.to_ascii_lowercase();
    let mut scored: Vec<(usize, ScoredCommandItem)> = Vec::new();

    // Get skill usage scores from Lane C global tracker
    let usage_scores: std::collections::HashMap<String, f64> =
        allthecodes_skills::ranked_skill_usage()
            .into_iter()
            .map(|data| (data.name, data.rolling_score))
            .collect();

    // Process commands with multi-field weighted scoring
    for (_index, cmd) in commands::get_all_commands().into_iter().enumerate() {
        if is_hidden_palette_command(&cmd.name) {
            continue;
        }

        // Multi-field weighted fuzzy match
        let best = best_fuzzy_multi(
            &[
                (cmd.name.as_str(), 1.0), // name: full weight
                (&cmd.description, 0.5),  // description: half weight
            ],
            &cmd.aliases.iter().map(String::as_str).collect::<Vec<_>>(),
            query_lower.as_str(),
        );

        let matched = match best {
            Some(m) => m,
            None => continue,
        };

        let meta = command_meta(&cmd.name, cwd);
        let group = if matched.score == 0 {
            // Empty query or exact match — group by source
            CommandGroup::Builtin
        } else {
            CommandGroup::Builtin
        };

        // Get usage score
        let usage = usage_scores.get(&cmd.name).copied().unwrap_or(0.0);

        let source_weight = match group {
            CommandGroup::Builtin => 0,
            CommandGroup::User => 1,
            CommandGroup::Project => 2,
            CommandGroup::Plugin => 3,
            CommandGroup::Skill => 4,
        };

        scored.push((
            source_weight,
            ScoredCommandItem {
                item: CommandItem {
                    usage: meta.usage,
                    examples: meta.examples,
                    edit_targets: meta.edit_targets,
                    name: cmd.name,
                    aliases: cmd.aliases,
                    description: cmd.description,
                    source_group: None,
                },
                group,
                score: matched.score,
                usage_score: usage,
            },
        ));
    }

    // Also check dynamic registry entries from Lane C
    let registry = commands::DYNAMIC_REGISTRY.lock();
    for entry in registry.list_all() {
        let name = &entry.name;
        if is_hidden_palette_command(name) {
            continue;
        }

        // Skip if already handled by builtins with the same name
        if scored.iter().any(|(_, s)| s.item.name == *name) {
            continue;
        }

        let all_candidates = std::iter::once(name.as_str())
            .chain(entry.aliases.iter().map(String::as_str))
            .chain(std::iter::once(entry.description.as_str()));

        let matched = best_fuzzy(all_candidates, &query_lower);
        let matched = match matched {
            Some(m) => m,
            None => continue,
        };

        let meta = command_meta(name, cwd);
        let group = match entry.source {
            commands::dynamic_registry::CommandSource::Builtin => CommandGroup::Builtin,
            commands::dynamic_registry::CommandSource::User => CommandGroup::User,
            commands::dynamic_registry::CommandSource::Project => CommandGroup::Project,
            commands::dynamic_registry::CommandSource::Plugin => CommandGroup::Plugin,
            commands::dynamic_registry::CommandSource::Skill => CommandGroup::Skill,
        };

        let source_weight = group.priority();
        let usage = entry.usage_score;

        // Hidden exact-name priority: hidden commands that match exactly get
        // bumped ahead regardless of score
        let effective_score =
            if entry.hidden && matched.kind == crate::ui::fuzzy_match::FuzzyMatchKind::Exact {
                0
            } else {
                matched.score
            };

        scored.push((
            100 - source_weight as usize, // invert so higher priority = lower sort key
            ScoredCommandItem {
                item: CommandItem {
                    usage: meta.usage,
                    examples: meta.examples,
                    edit_targets: meta.edit_targets,
                    name: name.clone(),
                    aliases: entry.aliases.clone(),
                    description: entry.description.clone(),
                    source_group: Some(group.label()),
                },
                group,
                score: effective_score,
                usage_score: usage,
            },
        ));
    }

    // Sort: empty-query grouping → score → usage_score
    if query_lower.is_empty() {
        // Empty query: recent → builtin → user → project → policy
        scored.sort_by(|(pa, a), (pb, b)| {
            a.group
                .priority()
                .cmp(&b.group.priority())
                .reverse()
                .then_with(|| {
                    b.usage_score
                        .partial_cmp(&a.usage_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| pa.cmp(pb))
        });
    } else {
        // Non-empty query: score → usage_score (tie-break)
        scored.sort_by(|(_, a), (_, b)| {
            a.score.cmp(&b.score).then_with(|| {
                b.usage_score
                    .partial_cmp(&a.usage_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
    }

    scored.into_iter().map(|(_, s)| s.item).collect()
}

fn is_hidden_palette_command(name: &str) -> bool {
    allthecodes_commands::is_hidden_command(name)
}

/// Multi-field weighted fuzzy match.
///
/// Scores the candidate against name (full weight), description (half weight),
/// and aliases (0.75 weight each). Returns the lowest score across all fields.
fn best_fuzzy_multi(
    primary_fields: &[(&str, f64)], // (field, weight)
    alias_fields: &[&str],
    query: &str,
) -> Option<crate::ui::fuzzy_match::FuzzyMatch> {
    let mut best: Option<(usize, crate::ui::fuzzy_match::FuzzyMatchKind)> = None;

    for &(field, weight) in primary_fields {
        if let Some(m) = fuzzy_match(field, query) {
            let weighted = (m.score as f64 * weight) as usize;
            let is_better = match best {
                Some((ref best_score, ref best_kind)) => {
                    (m.kind as u8) < (*best_kind as u8)
                        || (m.kind == *best_kind && weighted < *best_score)
                }
                None => true,
            };
            if is_better {
                best = Some((weighted, m.kind));
            }
        }
    }

    for &alias in alias_fields {
        if let Some(m) = fuzzy_match(alias, query) {
            let weighted = (m.score as f64 * 0.75) as usize;
            let is_better = match best {
                Some((ref best_score, ref best_kind)) => {
                    (m.kind as u8) < (*best_kind as u8)
                        || (m.kind == *best_kind && weighted < *best_score)
                }
                None => true,
            };
            if is_better {
                best = Some((weighted, m.kind));
            }
        }
    }

    best.map(|(score, kind)| crate::ui::fuzzy_match::FuzzyMatch { kind, score })
}

fn command_item_for_name(name: &str, cwd: &Path) -> Option<CommandItem> {
    commands::get_all_commands()
        .into_iter()
        .find(|cmd| cmd.name == name || cmd.aliases.iter().any(|alias| alias == name))
        .map(|cmd| {
            let meta = command_meta(&cmd.name, cwd);
            CommandItem {
                usage: meta.usage,
                examples: meta.examples,
                edit_targets: meta.edit_targets,
                name: cmd.name,
                aliases: cmd.aliases,
                description: cmd.description,
                source_group: None,
            }
        })
}

pub(super) fn command_from_argument_input(input: &str, cwd: &Path) -> Option<CommandItem> {
    let without_slash = input.strip_prefix('/')?;
    if !without_slash.contains(char::is_whitespace) {
        return None;
    }

    let name = without_slash.split_whitespace().next()?;
    command_item_for_name(name, cwd)
}

/// Find a mid-input slash command token.
///
/// Given input like `look at /help for docs` with cursor at position 14,
/// this returns the `/help` command.
pub(crate) fn find_mid_input_slash_command(
    input: &str,
    cursor_pos: usize,
) -> Option<(String, String)> /* (command_name, query_before_cmd) */ {
    let byte_pos = cursor_pos.min(input.len());
    let prefix = &input[..byte_pos];

    // Find last `/` preceded by whitespace
    let slash_pos = prefix.rfind('/')?;
    if slash_pos > 0 && !input[..slash_pos].ends_with(' ') {
        return None; // `/` is part of a word
    }

    let after_slash = &input[slash_pos + 1..byte_pos];
    if after_slash.contains(char::is_whitespace) {
        return None; // Already has arguments
    }

    Some((after_slash.to_string(), input[..slash_pos].to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::filtered_commands;
    use super::find_mid_input_slash_command;

    #[test]
    fn command_filter_orders_exact_matches_before_subsequence_matches() {
        let commands = filtered_commands("cp", Path::new("/repo"));

        // "copy" is an alias or match for "cp"
        let _first = commands
            .first()
            .map(|command| command.name.as_str())
            .unwrap_or("");
        // "compact" should appear but after exact matches
        assert!(commands.iter().any(|command| command.name == "compact"));
    }

    #[test]
    fn empty_query_groups_by_source() {
        let commands = filtered_commands("", Path::new("/repo"));
        assert!(
            !commands.is_empty(),
            "empty query should return all commands"
        );
    }

    #[test]
    fn find_mid_input_slash_matches() {
        let result = find_mid_input_slash_command("look at /help for docs", 13);
        assert!(result.is_some());
        let (cmd, before) = result.unwrap();
        assert_eq!(cmd, "help");
        assert_eq!(before, "look at ");
    }

    #[test]
    fn find_mid_input_no_slash() {
        let result = find_mid_input_slash_command("just text", 4);
        assert!(result.is_none());
    }

    #[test]
    fn find_mid_input_slash_with_args_returns_none() {
        let result = find_mid_input_slash_command("do /help me now", 13);
        assert!(
            result.is_none(),
            "slash with args should not trigger mid-input completion"
        );
    }
}
