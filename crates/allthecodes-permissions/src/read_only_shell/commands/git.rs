use std::collections::HashMap;

use super::super::flag_validation::{map_from_pairs, merge_maps};
use super::super::types::{ExternalCommandConfig, FlagArgType};

fn git_ref_selection_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--all", FlagArgType::None),
        ("--branches", FlagArgType::None),
        ("--tags", FlagArgType::None),
        ("--remotes", FlagArgType::None),
    ])
}

fn git_date_filter_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--since", FlagArgType::String),
        ("--after", FlagArgType::String),
        ("--until", FlagArgType::String),
        ("--before", FlagArgType::String),
    ])
}

fn git_log_display_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--oneline", FlagArgType::None),
        ("--graph", FlagArgType::None),
        ("--decorate", FlagArgType::None),
        ("--no-decorate", FlagArgType::None),
        ("--date", FlagArgType::String),
        ("--relative-date", FlagArgType::None),
    ])
}

fn git_count_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--max-count", FlagArgType::Number),
        ("-n", FlagArgType::Number),
    ])
}

fn git_stat_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--stat", FlagArgType::None),
        ("--numstat", FlagArgType::None),
        ("--shortstat", FlagArgType::None),
        ("--name-only", FlagArgType::None),
        ("--name-status", FlagArgType::None),
    ])
}

fn git_color_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--color", FlagArgType::None),
        ("--no-color", FlagArgType::None),
    ])
}

fn git_patch_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--patch", FlagArgType::None),
        ("-p", FlagArgType::None),
        ("--no-patch", FlagArgType::None),
        ("--no-ext-diff", FlagArgType::None),
        ("-s", FlagArgType::None),
    ])
}

fn git_author_filter_flags() -> HashMap<String, FlagArgType> {
    map_from_pairs(&[
        ("--author", FlagArgType::String),
        ("--committer", FlagArgType::String),
        ("--grep", FlagArgType::String),
    ])
}

pub(crate) fn make_git_read_only_commands() -> HashMap<String, ExternalCommandConfig> {
    let mut m = HashMap::new();

    // git diff
    m.insert(
        "git diff".into(),
        ExternalCommandConfig::new(merge_maps(&[
            git_stat_flags(),
            git_color_flags(),
            map_from_pairs(&[
                ("--dirstat", FlagArgType::None),
                ("--summary", FlagArgType::None),
                ("--patch-with-stat", FlagArgType::None),
                ("--word-diff", FlagArgType::None),
                ("--word-diff-regex", FlagArgType::String),
                ("--color-words", FlagArgType::None),
                ("--no-renames", FlagArgType::None),
                ("--check", FlagArgType::None),
                ("--ws-error-highlight", FlagArgType::String),
                ("--full-index", FlagArgType::None),
                ("--binary", FlagArgType::None),
                ("--abbrev", FlagArgType::Number),
                ("--break-rewrites", FlagArgType::None),
                ("--find-renames", FlagArgType::None),
                ("--find-copies", FlagArgType::None),
                ("--find-copies-harder", FlagArgType::None),
                ("--irreversible-delete", FlagArgType::None),
                ("--diff-algorithm", FlagArgType::String),
                ("--histogram", FlagArgType::None),
                ("--patience", FlagArgType::None),
                ("--minimal", FlagArgType::None),
                ("--ignore-space-at-eol", FlagArgType::None),
                ("--ignore-space-change", FlagArgType::None),
                ("--ignore-all-space", FlagArgType::None),
                ("--ignore-blank-lines", FlagArgType::None),
                ("--inter-hunk-context", FlagArgType::Number),
                ("--function-context", FlagArgType::None),
                ("--exit-code", FlagArgType::None),
                ("--quiet", FlagArgType::None),
                ("--cached", FlagArgType::None),
                ("--staged", FlagArgType::None),
                ("--pickaxe-regex", FlagArgType::None),
                ("--pickaxe-all", FlagArgType::None),
                ("--no-index", FlagArgType::None),
                ("--relative", FlagArgType::String),
                ("--diff-filter", FlagArgType::String),
                ("-u", FlagArgType::None),
                ("-M", FlagArgType::None),
                ("-C", FlagArgType::None),
                ("-B", FlagArgType::None),
                ("-D", FlagArgType::None),
                ("-l", FlagArgType::Number),
                ("-S", FlagArgType::String),
                ("-G", FlagArgType::String),
                ("-O", FlagArgType::String),
                ("-R", FlagArgType::None),
            ]),
        ])),
    );

    // git log
    m.insert(
        "git log".into(),
        ExternalCommandConfig::new(merge_maps(&[
            git_log_display_flags(),
            git_ref_selection_flags(),
            git_date_filter_flags(),
            git_count_flags(),
            git_stat_flags(),
            git_color_flags(),
            git_patch_flags(),
            git_author_filter_flags(),
            map_from_pairs(&[
                ("--abbrev-commit", FlagArgType::None),
                ("--full-history", FlagArgType::None),
                ("--dense", FlagArgType::None),
                ("--sparse", FlagArgType::None),
                ("--simplify-merges", FlagArgType::None),
                ("--ancestry-path", FlagArgType::None),
                ("--source", FlagArgType::None),
                ("--first-parent", FlagArgType::None),
                ("--merges", FlagArgType::None),
                ("--no-merges", FlagArgType::None),
                ("--reverse", FlagArgType::None),
                ("--walk-reflogs", FlagArgType::None),
                ("--skip", FlagArgType::Number),
                ("--max-age", FlagArgType::Number),
                ("--min-age", FlagArgType::Number),
                ("--author-date-order", FlagArgType::None),
                ("--topo-order", FlagArgType::None),
                ("--date-order", FlagArgType::None),
                ("--follow", FlagArgType::None),
                ("--no-walk", FlagArgType::None),
                ("--left-right", FlagArgType::None),
                ("--cherry-mark", FlagArgType::None),
                ("--cherry-pick", FlagArgType::None),
                ("--boundary", FlagArgType::None),
                ("--pretty", FlagArgType::String),
                ("--format", FlagArgType::String),
                ("--diff-filter", FlagArgType::String),
                ("-S", FlagArgType::String),
                ("-G", FlagArgType::String),
            ]),
        ])),
    );

    // git show
    m.insert(
        "git show".into(),
        ExternalCommandConfig::new(merge_maps(&[
            git_log_display_flags(),
            git_stat_flags(),
            git_color_flags(),
            git_patch_flags(),
            map_from_pairs(&[
                ("--abbrev-commit", FlagArgType::None),
                ("--word-diff", FlagArgType::None),
                ("--word-diff-regex", FlagArgType::String),
                ("--color-words", FlagArgType::None),
                ("--pretty", FlagArgType::String),
                ("--format", FlagArgType::String),
                ("--first-parent", FlagArgType::None),
                ("--raw", FlagArgType::None),
                ("--diff-filter", FlagArgType::String),
                ("-m", FlagArgType::None),
                ("--quiet", FlagArgType::None),
            ]),
        ])),
    );

    // git shortlog
    m.insert(
        "git shortlog".into(),
        ExternalCommandConfig::new(merge_maps(&[
            git_ref_selection_flags(),
            git_date_filter_flags(),
            map_from_pairs(&[
                ("-s", FlagArgType::None),
                ("--summary", FlagArgType::None),
                ("-n", FlagArgType::None),
                ("--numbered", FlagArgType::None),
                ("-e", FlagArgType::None),
                ("--email", FlagArgType::None),
                ("-c", FlagArgType::None),
                ("--committer", FlagArgType::None),
                ("--group", FlagArgType::String),
                ("--format", FlagArgType::String),
                ("--no-merges", FlagArgType::None),
                ("--author", FlagArgType::String),
            ]),
        ])),
    );

    // git reflog
    m.insert(
        "git reflog".into(),
        ExternalCommandConfig::with_check(
            merge_maps(&[
                git_log_display_flags(),
                git_ref_selection_flags(),
                git_date_filter_flags(),
                git_count_flags(),
                git_author_filter_flags(),
            ]),
            |_raw: &str, args: &[String]| {
                let dangerous = ["expire", "delete", "exists"];
                for token in args {
                    if token.is_empty() || token.starts_with('-') {
                        continue;
                    }
                    if dangerous.contains(&token.as_str()) {
                        return true;
                    }
                    return false;
                }
                false
            },
        ),
    );

    // git stash list
    m.insert(
        "git stash list".into(),
        ExternalCommandConfig::new(merge_maps(&[
            git_log_display_flags(),
            git_ref_selection_flags(),
            git_count_flags(),
        ])),
    );

    // git ls-remote
    m.insert(
        "git ls-remote".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--branches", FlagArgType::None),
            ("-b", FlagArgType::None),
            ("--tags", FlagArgType::None),
            ("-t", FlagArgType::None),
            ("--heads", FlagArgType::None),
            ("--refs", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("--sort", FlagArgType::String),
        ])),
    );

    // git status
    m.insert(
        "git status".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-s", FlagArgType::None),
            ("--short", FlagArgType::None),
            ("-b", FlagArgType::None),
            ("--branch", FlagArgType::None),
            ("--porcelain", FlagArgType::None),
            ("--long", FlagArgType::None),
            ("-v", FlagArgType::None),
            ("--verbose", FlagArgType::None),
            ("-u", FlagArgType::None),
            ("--untracked-files", FlagArgType::String),
            ("--ignore-submodules", FlagArgType::String),
            ("--column", FlagArgType::None),
            ("--no-column", FlagArgType::None),
            ("--renames", FlagArgType::None),
            ("--no-renames", FlagArgType::None),
        ])),
    );

    // git branch (read-only subcommands only)
    m.insert(
        "git branch".into(),
        ExternalCommandConfig::with_check(
            map_from_pairs(&[
                ("-a", FlagArgType::None),
                ("--all", FlagArgType::None),
                ("-r", FlagArgType::None),
                ("--remotes", FlagArgType::None),
                ("--list", FlagArgType::None),
                ("--show-current", FlagArgType::None),
                ("-v", FlagArgType::None),
                ("-vv", FlagArgType::None),
                ("--verbose", FlagArgType::None),
                ("--merged", FlagArgType::String),
                ("--no-merged", FlagArgType::String),
                ("--sort", FlagArgType::String),
                ("--format", FlagArgType::String),
                ("--color", FlagArgType::None),
                ("--no-color", FlagArgType::None),
                ("-q", FlagArgType::None),
                ("--quiet", FlagArgType::None),
            ]),
            |_raw: &str, args: &[String]| {
                // Block write subcommands: -d (delete), -D, -m (move), -M, -c (copy), -C
                // Block positional args that start with + (create)
                for token in args {
                    if token.is_empty() {
                        continue;
                    }
                    if token.starts_with('-') {
                        match token.as_str() {
                            "-d" | "-D" | "-m" | "-M" | "-c" | "-C" => return true,
                            _ => continue,
                        }
                    }
                    // First non-flag positional could be a branch name (safe to list)
                    // but also could be a creation: `git branch new-branch`
                    // We can't safely distinguish, so block.
                    return true;
                }
                false
            },
        ),
    );

    // git ls-files
    m.insert(
        "git ls-files".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-c", FlagArgType::None),
            ("--cached", FlagArgType::None),
            ("-m", FlagArgType::None),
            ("--modified", FlagArgType::None),
            ("-d", FlagArgType::None),
            ("--deleted", FlagArgType::None),
            ("-o", FlagArgType::None),
            ("--others", FlagArgType::None),
            ("-i", FlagArgType::None),
            ("--ignored", FlagArgType::None),
            ("-s", FlagArgType::None),
            ("--stage", FlagArgType::None),
            ("-u", FlagArgType::None),
            ("--unmerged", FlagArgType::None),
            ("-k", FlagArgType::None),
            ("--killed", FlagArgType::None),
            ("-z", FlagArgType::None),
            ("-t", FlagArgType::None),
            ("--exclude-standard", FlagArgType::None),
            ("--directory", FlagArgType::None),
            ("--no-empty-directory", FlagArgType::None),
            ("--full-name", FlagArgType::None),
            ("--recurse-submodules", FlagArgType::None),
            ("--error-unmatch", FlagArgType::None),
        ])),
    );

    // git describe
    m.insert(
        "git describe".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--all", FlagArgType::None),
            ("--tags", FlagArgType::None),
            ("--contains", FlagArgType::None),
            ("--abbrev", FlagArgType::Number),
            ("--candidates", FlagArgType::Number),
            ("--exact-match", FlagArgType::None),
            ("--debug", FlagArgType::None),
            ("--long", FlagArgType::None),
            ("--match", FlagArgType::String),
            ("--exclude", FlagArgType::String),
            ("--always", FlagArgType::None),
            ("--dirty", FlagArgType::None),
            ("--broken", FlagArgType::String),
        ])),
    );

    // git blame
    m.insert(
        "git blame".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-b", FlagArgType::None),
            ("--root", FlagArgType::None),
            ("--show-stats", FlagArgType::None),
            ("-L", FlagArgType::String),
            ("-l", FlagArgType::None),
            ("-t", FlagArgType::None),
            ("-s", FlagArgType::None),
            ("-w", FlagArgType::None),
            ("--line-porcelain", FlagArgType::None),
            ("--incremental", FlagArgType::None),
            ("--encoding", FlagArgType::String),
            ("--contents", FlagArgType::String),
            ("--date", FlagArgType::String),
            ("--ignore-rev", FlagArgType::String),
            ("--ignore-revs-file", FlagArgType::String),
            ("--color-lines", FlagArgType::None),
            ("--color-by-age", FlagArgType::None),
        ])),
    );

    // git grep
    m.insert(
        "git grep".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("--cached", FlagArgType::None),
            ("--no-index", FlagArgType::None),
            ("--untracked", FlagArgType::None),
            ("--no-exclude-standard", FlagArgType::None),
            ("--exclude-standard", FlagArgType::None),
            ("--recurse-submodules", FlagArgType::None),
            ("--parent-basename", FlagArgType::None),
            ("-v", FlagArgType::None),
            ("--invert-match", FlagArgType::None),
            ("-i", FlagArgType::None),
            ("--ignore-case", FlagArgType::None),
            ("-w", FlagArgType::None),
            ("--word-regexp", FlagArgType::None),
            ("-E", FlagArgType::None),
            ("--extended-regexp", FlagArgType::None),
            ("-F", FlagArgType::None),
            ("--fixed-strings", FlagArgType::None),
            ("-G", FlagArgType::None),
            ("--basic-regexp", FlagArgType::None),
            ("-P", FlagArgType::None),
            ("--perl-regexp", FlagArgType::None),
            ("-n", FlagArgType::None),
            ("--line-number", FlagArgType::None),
            ("-h", FlagArgType::None),
            ("-o", FlagArgType::None),
            ("--only-matching", FlagArgType::None),
            ("--column", FlagArgType::None),
            ("-l", FlagArgType::None),
            ("--files-with-matches", FlagArgType::None),
            ("-L", FlagArgType::None),
            ("--files-without-match", FlagArgType::None),
            ("-q", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("--count", FlagArgType::None),
            ("--all-match", FlagArgType::None),
            ("-A", FlagArgType::Number),
            ("--after-context", FlagArgType::Number),
            ("-B", FlagArgType::Number),
            ("--before-context", FlagArgType::Number),
            ("-C", FlagArgType::Number),
            ("--context", FlagArgType::Number),
            ("-W", FlagArgType::None),
            ("--funcname", FlagArgType::None),
            ("--function-context", FlagArgType::None),
            ("--max-count", FlagArgType::Number),
            ("--max-depth", FlagArgType::Number),
            ("--break", FlagArgType::None),
            ("--heading", FlagArgType::None),
            ("-p", FlagArgType::None),
            ("--show-function", FlagArgType::None),
            ("--color", FlagArgType::String),
            ("--no-color", FlagArgType::None),
            ("--color-matching", FlagArgType::None),
            ("--color-matched", FlagArgType::None),
            ("--no-full-name", FlagArgType::None),
            ("--full-name", FlagArgType::None),
            ("--threads", FlagArgType::Number),
            ("-f", FlagArgType::String),
            ("--file", FlagArgType::String),
            ("-e", FlagArgType::String),
            ("--and", FlagArgType::None),
            ("--or", FlagArgType::None),
            ("--not", FlagArgType::None),
            ("--open-files-in-pager", FlagArgType::None),
        ])),
    );

    // git config (read-only)
    m.insert(
        "git config".into(),
        ExternalCommandConfig::with_check(
            map_from_pairs(&[
                ("--list", FlagArgType::None),
                ("-l", FlagArgType::None),
                ("--global", FlagArgType::None),
                ("--system", FlagArgType::None),
                ("--local", FlagArgType::None),
                ("--worktree", FlagArgType::None),
                ("--file", FlagArgType::String),
                ("-f", FlagArgType::String),
                ("--blob", FlagArgType::String),
                ("--get", FlagArgType::None),
                ("--get-all", FlagArgType::None),
                ("--get-color", FlagArgType::None),
                ("--get-colorbool", FlagArgType::None),
                ("--get-urlmatch", FlagArgType::None),
                ("--bool", FlagArgType::None),
                ("--int", FlagArgType::None),
                ("--bool-or-int", FlagArgType::None),
                ("--path", FlagArgType::None),
                ("--type", FlagArgType::String),
                ("--show-origin", FlagArgType::None),
                ("--show-scope", FlagArgType::None),
                ("--name-only", FlagArgType::None),
                ("-z", FlagArgType::None),
                ("--null", FlagArgType::None),
                ("--includes", FlagArgType::None),
                ("--no-includes", FlagArgType::None),
                ("--default", FlagArgType::String),
                ("--comment", FlagArgType::String),
                ("--fixed-value", FlagArgType::None),
            ]),
            |_raw: &str, args: &[String]| {
                // Block if any positional looks like setting a value: `git config key value`
                // Read-only allows: `git config --get key`, `git config --list`, `git config key` (shows value)
                // Block: any positional after key name that doesn't start with -
                let mut positional_count = 0u32;
                for token in args {
                    if token.is_empty() || token.starts_with('-') {
                        continue;
                    }
                    positional_count += 1;
                    if positional_count > 1 {
                        return true; // Setting a value
                    }
                }
                false
            },
        ),
    );

    // git tag (read-only subcommands)
    m.insert(
        "git tag".into(),
        ExternalCommandConfig::with_check(
            map_from_pairs(&[
                ("-l", FlagArgType::None),
                ("--list", FlagArgType::None),
                ("-n", FlagArgType::Number),
                ("--sort", FlagArgType::String),
                ("--format", FlagArgType::String),
                ("--color", FlagArgType::None),
                ("--no-color", FlagArgType::None),
                ("--merged", FlagArgType::String),
                ("--no-merged", FlagArgType::String),
                ("--contains", FlagArgType::String),
                ("--no-contains", FlagArgType::String),
                ("--points-at", FlagArgType::String),
                ("--column", FlagArgType::None),
            ]),
            |_raw: &str, args: &[String]| {
                // Block if any positional looks like creating/deleting a tag
                for token in args {
                    if token.is_empty() || token.starts_with('-') {
                        continue;
                    }
                    // First non-flag could be a tag name to list, or to create.
                    // Without -l/--list, creation is implied. Block.
                    if !args.iter().any(|a| a == "-l" || a == "--list") {
                        return true;
                    }
                    return false;
                }
                false
            },
        ),
    );

    // git help
    m.insert(
        "git help".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-a", FlagArgType::None),
            ("--all", FlagArgType::None),
            ("-g", FlagArgType::None),
            ("--guides", FlagArgType::None),
            ("-i", FlagArgType::None),
            ("--info", FlagArgType::None),
            ("-m", FlagArgType::None),
            ("--man", FlagArgType::None),
            ("-w", FlagArgType::None),
            ("--web", FlagArgType::None),
        ])),
    );

    // git remote (read-only)
    m.insert(
        "git remote".into(),
        ExternalCommandConfig::with_check(
            map_from_pairs(&[("-v", FlagArgType::None), ("--verbose", FlagArgType::None)]),
            |_raw: &str, args: &[String]| {
                let write_subcommands = [
                    "add",
                    "rename",
                    "remove",
                    "set-head",
                    "set-branches",
                    "set-url",
                    "prune",
                    "update",
                ];
                for token in args {
                    if token.is_empty() || token.starts_with('-') {
                        continue;
                    }
                    if write_subcommands.contains(&token.as_str()) {
                        return true;
                    }
                    // `git remote` or `git remote -v` or `git remote show origin`
                    if token == "show" {
                        return false; // `show` is safe
                    }
                    return false; // unknown positional, could be safe (a remote name to show)
                }
                false
            },
        ),
    );

    m
}
