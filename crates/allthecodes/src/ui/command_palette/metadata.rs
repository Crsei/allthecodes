use std::path::Path;

use allthecodes_config::paths as cfg_paths;

use super::edit_targets::EditTarget;

pub(super) struct CommandMeta {
    pub(super) usage: String,
    pub(super) examples: Vec<String>,
    pub(super) edit_targets: Vec<EditTarget>,
}

fn simple_meta(usage: &str, examples: &[&str]) -> CommandMeta {
    CommandMeta {
        usage: usage.to_string(),
        examples: examples
            .iter()
            .map(|example| (*example).to_string())
            .collect(),
        edit_targets: Vec::new(),
    }
}

pub(super) fn command_meta(name: &str, cwd: &Path) -> CommandMeta {
    match name {
        "add-dir" => simple_meta("/add-dir <path>", &["/add-dir ../shared"]),
        "advisor" => simple_meta(
            "/advisor <model|clear|off>",
            &["/advisor MOTA", "/advisor clear"],
        ),
        "agents" => simple_meta(
            "/agents <list|show> [name]",
            &["/agents", "/agents show executor"],
        ),
        "assistant" => simple_meta("/assistant", &["/assistant"]),
        "audit-export" => simple_meta("/audit-export", &["/audit-export"]),
        "branch" => simple_meta("/branch", &["/branch"]),
        "btw" => simple_meta("/btw <question>", &["/btw explain the last error"]),
        "channels" => simple_meta("/channels [list|status]", &["/channels status"]),
        "chrome" => simple_meta("/chrome [status|reconnect|help]", &["/chrome reconnect"]),
        "clear" => simple_meta("/clear", &["/clear"]),
        "commit" => simple_meta("/commit [message]", &["/commit fix command palette layout"]),
        "compact" => simple_meta("/compact [instructions]", &["/compact focus on UI fixes"]),
        "context" => simple_meta("/context [json|raw|tui|full]", &["/context json"]),
        "copy" => simple_meta("/copy", &["/copy"]),
        "cost" => simple_meta("/cost", &["/cost"]),
        "daemon" => simple_meta("/daemon [status|stop]", &["/daemon status"]),
        "debug" => simple_meta("/debug snapshot", &["/debug snapshot"]),
        "diff" => simple_meta("/diff", &["/diff"]),
        "doctor" => simple_meta("/doctor [summary|raw]", &["/doctor summary"]),
        "dream" => simple_meta("/dream [--days N]", &["/dream --days 7", "/logs"]),
        "effort" => simple_meta(
            "/effort <low|medium|high|auto|max|token-count>",
            &["/effort", "/effort medium"],
        ),
        "exit" => simple_meta("/exit", &["/exit", "/quit"]),
        "export" => simple_meta(
            "/export [list|path|session-id]",
            &["/export", "/export session.md"],
        ),
        "experimental" => simple_meta(
            "/experimental [status|list|on|off|reset]",
            &["/experimental status", "/experimental on"],
        ),
        "extra-usage" => simple_meta("/extra-usage", &["/extra-usage"]),
        "fast" => simple_meta("/fast [on|off|status]", &["/fast on"]),
        "files" => simple_meta("/files", &["/files"]),
        "gbranch" => simple_meta("/gbranch [branch-name]", &["/gbranch feature/ui-fix"]),
        "goal" => simple_meta(
            "/goal [--tokens N] <objective>",
            &["/goal --tokens 20000 ship the release", "/goal status"],
        ),
        "help" => simple_meta("/help [command]", &["/help", "/help keybindings"]),
        "hooks" => CommandMeta {
            usage: "/hooks <list|path|open> [event|layer]".to_string(),
            examples: vec![
                "/hooks list PreToolUse".to_string(),
                "/hooks open project".to_string(),
            ],
            edit_targets: vec![
                EditTarget::new(
                    "managed",
                    allthecodes_config::settings::managed_settings_path(),
                    cwd,
                    "open managed",
                ),
                EditTarget::new(
                    "user",
                    allthecodes_config::settings::user_settings_path(),
                    cwd,
                    "open user",
                ),
                EditTarget::new(
                    "project",
                    allthecodes_config::settings::project_settings_path(cwd),
                    cwd,
                    "open project",
                ),
                EditTarget::new(
                    "local",
                    allthecodes_config::settings::local_settings_path(cwd),
                    cwd,
                    "open local",
                ),
            ],
        },
        "ide" => simple_meta(
            "/ide [detect|status|select|clear|reconnect]",
            &["/ide status"],
        ),
        "init" => simple_meta("/init", &["/init"]),
        "kairos" => simple_meta(
            "/kairos [status|enable|disable|start|stop|restart|feature|bridge]",
            &["/kairos", "/kairos enable --start", "/kairos status"],
        ),
        "insights" => simple_meta("/insights [fast|full]", &["/insights"]),
        "login" => simple_meta(
            "/login [claude-code|claude-ai|console|codex|codex-oauth|codex-cli|custom|openai-api|bedrock|vertex]",
            &[
                "/login claude-code",
                "/login codex",
                "/login custom",
                "/login bedrock",
                "/login vertex",
            ],
        ),
        "login-code" => simple_meta("/login-code <authorization-code>", &["/login-code abc123"]),
        "logout" => simple_meta("/logout", &["/logout"]),
        "lsp" => simple_meta(
            "/lsp [status|servers|recommendations]",
            &["/lsp status", "/lsp recommendations"],
        ),
        "loop" => simple_meta(
            "/loop <interval|list|remove|trigger|pause|resume> ...",
            &["/loop 5m /status"],
        ),
        "mcp" => CommandMeta {
            usage: "/mcp <list|status|add|edit|remove|approve|reject|connect|disconnect|reconnect>"
                .to_string(),
            examples: vec![
                "/mcp add ctx7 --command=npx --arg=-y --arg=@upstash/context7-mcp".to_string(),
                "/mcp approve playwright --all-project".to_string(),
            ],
            edit_targets: vec![
                EditTarget::new(
                    "user",
                    allthecodes_config::settings::user_settings_path(),
                    cwd,
                    "edit user",
                ),
                EditTarget::new(
                    "project",
                    allthecodes_config::settings::project_settings_path(cwd),
                    cwd,
                    "edit project",
                ),
                EditTarget::new(
                    "local",
                    allthecodes_config::settings::local_settings_path(cwd),
                    cwd,
                    "edit local",
                ),
            ],
        },
        "model" => simple_meta("/model [model-id|alias]", &["/model MOTA"]),
        "model-add" => simple_meta(
            "/model-add <name> [input_price output_price]",
            &["/model-add gpt-4o 2.50 10.00"],
        ),
        "notify" => simple_meta("/notify [status|test|on|off]", &["/notify status"]),
        "permissions" => simple_meta(
            "/permissions [mode|allow|ask|deny|session-grant|clear-session-grants|reset] ...",
            &["/permissions", "/permissions mode plan"],
        ),
        "plan" => CommandMeta {
            usage: "/plan [enter|show|status|approve|reject|link|classify] ...".to_string(),
            examples: vec![
                "/plan".to_string(),
                "/plan status".to_string(),
                "/plan enter refactor command palette".to_string(),
            ],
            edit_targets: vec![EditTarget::new(
                "current",
                cfg_paths::current_plan_file_path(cwd),
                cwd,
                "open",
            )],
        },
        "plugin" => CommandMeta {
            usage: "/plugin <list|installed|disabled|errors|status|enable|disable|uninstall> [id]"
                .to_string(),
            examples: vec!["/plugin enable <plugin-id>".to_string()],
            edit_targets: vec![
                EditTarget::new(
                    "installed",
                    allthecodes_config::paths::plugins_dir().join("installed_plugins.json"),
                    cwd,
                    "",
                ),
                EditTarget::new(
                    "cache",
                    allthecodes_config::paths::plugins_dir().join("cache"),
                    cwd,
                    "",
                ),
            ],
        },
        "rate-limit-options" => simple_meta("/rate-limit-options", &["/rate-limit-options"]),
        "recap" => simple_meta("/recap [short|long]", &["/recap short"]),
        "reload-plugins" => simple_meta("/reload-plugins", &["/reload-plugins"]),
        "rename" => simple_meta("/rename [title|clear]", &["/rename UI command fixes"]),
        "remote" => simple_meta(
            "/remote <status|adapters|connect|test-message|runs|show|events|stop|doctor> ...",
            &["/remote status", "/remote connect telegram", "/remote runs"],
        ),
        "resume" => simple_meta(
            "/resume <session-id|recent>",
            &["/resume recent", "/preview"],
        ),
        "review" => simple_meta("/review <pr-number|url|branch>", &["/review 123"]),
        "rewind" => simple_meta("/rewind <turn|message-id>", &["/rewind 2"]),
        "sandbox" => simple_meta(
            "/sandbox <status|mode|network|paths> ...",
            &["/sandbox mode workspace", "/sandbox network off"],
        ),
        "schedule" => simple_meta(
            "/schedule <add|list|pause|trigger|remove> ...",
            &["/schedule add 30m /status"],
        ),
        "security-review" => simple_meta(
            "/security-review [scope]",
            &["/security-review crates/allthecodes/src/ui"],
        ),
        "session" => simple_meta("/session [list|list all]", &["/session list", "/sessions"]),
        "session-export" => simple_meta(
            "/session-export <list|session-id|path>",
            &["/session-export list", "/structured-export"],
        ),
        "simplify" => simple_meta(
            "/simplify [--single|-1] [scope]",
            &["/simplify --single crates/allthecodes/src/ui"],
        ),
        "skills" => CommandMeta {
            usage: "/skills [name]".to_string(),
            examples: vec!["/skills".to_string()],
            edit_targets: vec![
                EditTarget::new("user", allthecodes_config::paths::skills_dir_global(), cwd, ""),
                EditTarget::new("project", cwd.join(".allthecodes").join("skills"), cwd, ""),
            ],
        },
        "sleep" => simple_meta("/sleep <seconds>", &["/sleep 60"]),
        "status" => simple_meta("/status", &["/status"]),
        "statusline" => simple_meta(
            "/statusline <status|set|clear|enable|disable|test|payload|refresh|timeout|padding> ...",
            &["/statusline show"],
        ),
        "tasks" => simple_meta(
            "/tasks <list|show|stop|delete> [id]",
            &["/tasks", "/tasks show task-123"],
        ),
        "team" => simple_meta(
            "/team <status|list|create|spawn|send|kill|leave|delete> ...",
            &["/team create ui-fix", "/team status"],
        ),
        "team-onboarding" => simple_meta("/team-onboarding [team-name]", &["/team-onboarding"]),
        "terminal-setup" => {
            simple_meta("/terminal-setup [env|tips|all]", &["/terminal-setup tips"])
        }
        "version" => simple_meta("/version", &["/version"]),
        "voice" => simple_meta("/voice [status|on|off|toggle|diagnose]", &["/voice status"]),
        "keybindings" => CommandMeta {
            usage: "/keybindings [open|status|list|reload|path]".to_string(),
            examples: vec!["/keybindings".to_string()],
            edit_targets: vec![EditTarget::new(
                "user",
                allthecodes_config::paths::keybindings_path(),
                cwd,
                "open",
            )],
        },
        "config" => CommandMeta {
            usage: "/config <show|sources|schema|set|reset> [key] [value]".to_string(),
            examples: vec!["/config set theme dark".to_string()],
            edit_targets: vec![
                EditTarget::new(
                    "user",
                    allthecodes_config::settings::user_settings_path(),
                    cwd,
                    "--user",
                ),
                EditTarget::new(
                    "project",
                    allthecodes_config::settings::project_settings_path(cwd),
                    cwd,
                    "--project",
                ),
            ],
        },
        "memory" => CommandMeta {
            usage: "/memory <show|edit|add|search|open> [text|scope]".to_string(),
            examples: vec![
                "/memory search auth".to_string(),
                "/memory open global".to_string(),
            ],
            edit_targets: vec![
                EditTarget::new("project", cwd.join("AGENTS.md"), cwd, "open project"),
                EditTarget::new(
                    "global",
                    allthecodes_config::paths::memory_dir_global(),
                    cwd,
                    "open global",
                ),
            ],
        },
        other => CommandMeta {
            usage: format!("/{other}"),
            examples: vec![format!("/{other}")],
            edit_targets: Vec::new(),
        },
    }
}
