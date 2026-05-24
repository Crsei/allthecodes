// Phase 3: Permission system
//
// Permission modes: Default (ask), Auto (auto-approve with classifier), Bypass (skip all), Plan (read-only)
// Rule matching: allow/deny/ask rules grouped by source
// Dangerous command detection: rm -rf, git push --force, etc.
// Decision state machine: full flow from rules → hooks → mode

pub mod bash_matcher;
pub mod dangerous;
pub mod decision;
pub mod path_validation;
pub mod permission_update;
pub mod read_only_shell;
pub mod rules;
pub mod shadowed_rules;
