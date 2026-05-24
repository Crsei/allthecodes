//! Dynamic command registry supporting multiple sources (builtin, user/project,
//! plugin, skill). This is the central contract consumed by other lanes.
//!
//! The registry stores command metadata, priority-based lookup information, and
//! the execution strategy needed by the slash-command dispatcher.

use serde::{Deserialize, Serialize};

/// Source of a registered command — determines priority and display grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandSource {
    Builtin,
    User,
    Project,
    Plugin,
    Skill,
}

impl CommandSource {
    /// Priority value: higher = higher priority.
    ///
    /// Builtin commands always win. Plugin commands cannot shadow builtin.
    /// Skill commands cannot shadow plugin commands.
    pub const fn priority(&self) -> u8 {
        match self {
            CommandSource::Builtin => 100,
            CommandSource::User => 80,
            CommandSource::Project => 60,
            CommandSource::Plugin => 40,
            CommandSource::Skill => 20,
        }
    }
}

/// How a dynamically registered command is executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStrategy {
    /// Inline execution (builtin commands).
    Inline,
    /// User-invocable skill execution.
    Skill,
    /// Forked sub-agent execution (skills).
    Fork,
    /// Forward to plugin runtime.
    Plugin,
    /// Forward to MCP server.
    Mcp { server_name: String },
}

/// Metadata for a dynamically registered command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicCommandEntry {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub source: CommandSource,
    #[serde(default)]
    pub plugin_id: Option<String>,
    /// Don't show in listings but accept execution.
    pub hidden: bool,
    /// 0.0-1.0, higher = more used.
    pub usage_score: f64,
    pub execution_strategy: ExecutionStrategy,
}

/// The dynamic command registry.
///
/// Stores all command entries and provides priority-based lookup:
/// when multiple entries share the same name, the one with the highest
/// `CommandSource` priority wins.
pub struct DynamicRegistry {
    commands: Vec<DynamicCommandEntry>,
}

impl DynamicRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Register a command.
    ///
    /// If a command with the same name exists from the **same source**, it is
    /// updated and the previous entry is returned.
    ///
    /// Higher-priority sources shadow lower-priority ones during lookup
    /// (see [`find`](Self::find)), but all entries are retained so that
    /// unregistering a higher-priority source reveals the next best.
    ///
    /// Returns the previous entry if one was replaced (same name + same source).
    pub fn register(&mut self, entry: DynamicCommandEntry) -> Option<DynamicCommandEntry> {
        // Exact match (same name + same source) → replace
        if let Some(pos) = self
            .commands
            .iter()
            .position(|c| c.name == entry.name && c.source == entry.source)
        {
            let old = self.commands.remove(pos);
            self.commands.push(entry);
            return Some(old);
        }

        // New entry — just append
        self.commands.push(entry);
        None
    }

    /// Register multiple commands at once.
    ///
    /// Returns all previously-replaced entries.
    pub fn register_all(&mut self, entries: Vec<DynamicCommandEntry>) -> Vec<DynamicCommandEntry> {
        entries
            .into_iter()
            .filter_map(|entry| self.register(entry))
            .collect()
    }

    /// Unregister a command by name and source.
    pub fn unregister(&mut self, name: &str, source: CommandSource) -> Option<DynamicCommandEntry> {
        if let Some(pos) = self
            .commands
            .iter()
            .position(|c| c.name == name && c.source == source)
        {
            Some(self.commands.remove(pos))
        } else {
            None
        }
    }

    /// Find a command entry by exact name or alias match.
    ///
    /// Returns the highest-priority match when multiple sources have registered
    /// the same name.
    pub fn find(&self, name_or_alias: &str) -> Option<&DynamicCommandEntry> {
        let mut best: Option<&DynamicCommandEntry> = None;
        let mut best_priority: u8 = 0;

        for entry in &self.commands {
            let matches =
                entry.name == name_or_alias || entry.aliases.iter().any(|a| a == name_or_alias);
            if matches {
                let prio = entry.source.priority();
                if best.is_none() || prio > best_priority {
                    best = Some(entry);
                    best_priority = prio;
                }
            }
        }

        best
    }

    /// List all visible (non-hidden) commands, sorted by source priority then
    /// usage_score descending.
    pub fn list_visible(&self) -> Vec<&DynamicCommandEntry> {
        let mut result: Vec<&DynamicCommandEntry> =
            self.commands.iter().filter(|c| !c.hidden).collect();

        result.sort_by(|a, b| {
            b.source.priority().cmp(&a.source.priority()).then_with(|| {
                b.usage_score
                    .partial_cmp(&a.usage_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        result
    }

    /// List ALL commands including hidden ones (for execution lookup),
    /// sorted by source priority then usage_score descending.
    pub fn list_all(&self) -> Vec<&DynamicCommandEntry> {
        let mut result: Vec<&DynamicCommandEntry> = self.commands.iter().collect();

        result.sort_by(|a, b| {
            b.source.priority().cmp(&a.source.priority()).then_with(|| {
                b.usage_score
                    .partial_cmp(&a.usage_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        result
    }

    /// Update the usage score for a command (called after successful execution).
    ///
    /// Boosts the score by 0.1 toward 1.0. Idempotent within a single
    /// registry instance — the source-level priority in `find()` is unaffected.
    pub fn record_usage(&mut self, name: &str) {
        for entry in &mut self.commands {
            if entry.name == name {
                entry.usage_score = (entry.usage_score + 0.1).min(1.0);
            }
        }
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Returns the priority order of sources.
    ///
    /// Builtin (100) > User (80) > Project (60) > Plugin (40) > Skill (20).
    pub const fn source_priority(source: CommandSource) -> u8 {
        source.priority()
    }
}

impl Default for DynamicRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, source: CommandSource, usage_score: f64) -> DynamicCommandEntry {
        DynamicCommandEntry {
            name: name.to_string(),
            aliases: Vec::new(),
            description: String::new(),
            source,
            plugin_id: None,
            hidden: false,
            usage_score,
            execution_strategy: ExecutionStrategy::Inline,
        }
    }

    #[test]
    fn register_new_adds_entry() {
        let mut reg = DynamicRegistry::new();
        assert!(reg.is_empty());

        reg.register(entry("test", CommandSource::Builtin, 0.5));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.find("test").unwrap().name, "test");
    }

    #[test]
    fn register_same_source_replaces() {
        let mut reg = DynamicRegistry::new();

        let old = reg.register(entry("cmd", CommandSource::Builtin, 0.3));
        assert!(old.is_none());

        let old = reg.register(entry("cmd", CommandSource::Builtin, 0.9));
        assert!(old.is_some());
        assert_eq!(old.unwrap().usage_score, 0.3);
        assert_eq!(reg.len(), 1);
        assert!((reg.find("cmd").unwrap().usage_score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn register_different_source_appends() {
        let mut reg = DynamicRegistry::new();

        reg.register(entry("cmd", CommandSource::Builtin, 0.5));
        reg.register(entry("cmd", CommandSource::Plugin, 0.1));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn find_returns_highest_priority() {
        let mut reg = DynamicRegistry::new();

        reg.register(entry("cmd", CommandSource::Plugin, 0.5));
        reg.register(entry("cmd", CommandSource::Builtin, 0.3));

        let found = reg.find("cmd").unwrap();
        assert_eq!(found.source, CommandSource::Builtin);
    }

    #[test]
    fn find_with_alias() {
        let mut reg = DynamicRegistry::new();
        let mut e = entry("help", CommandSource::Builtin, 0.5);
        e.aliases = vec!["h".to_string(), "?".to_string()];
        reg.register(e);

        assert_eq!(reg.find("help").unwrap().name, "help");
        assert_eq!(reg.find("h").unwrap().name, "help");
        assert_eq!(reg.find("?").unwrap().name, "help");
        assert!(reg.find("missing").is_none());
    }

    #[test]
    fn unregister_removes_specific_source() {
        let mut reg = DynamicRegistry::new();

        reg.register(entry("cmd", CommandSource::Builtin, 0.5));
        reg.register(entry("cmd", CommandSource::Plugin, 0.2));
        assert_eq!(reg.len(), 2);

        let removed = reg.unregister("cmd", CommandSource::Plugin);
        assert!(removed.is_some());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.find("cmd").unwrap().source, CommandSource::Builtin);
    }

    #[test]
    fn list_visible_excludes_hidden() {
        let mut reg = DynamicRegistry::new();

        let mut e1 = entry("visible", CommandSource::Builtin, 0.5);
        e1.hidden = false;
        reg.register(e1);

        let mut e2 = entry("hidden", CommandSource::Plugin, 0.3);
        e2.hidden = true;
        reg.register(e2);

        let visible = reg.list_visible();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "visible");

        let all = reg.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn record_usage_increases_score() {
        let mut reg = DynamicRegistry::new();

        reg.register(entry("cmd", CommandSource::Builtin, 0.0));
        reg.record_usage("cmd");
        assert!((reg.find("cmd").unwrap().usage_score - 0.1).abs() < 1e-6);

        // Capped at 1.0
        for _ in 0..20 {
            reg.record_usage("cmd");
        }
        assert!((reg.find("cmd").unwrap().usage_score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn register_all_returns_replaced() {
        let mut reg = DynamicRegistry::new();
        reg.register(entry("a", CommandSource::Builtin, 0.5));

        let replaced = reg.register_all(vec![
            entry("a", CommandSource::Builtin, 0.9),
            entry("b", CommandSource::Plugin, 0.1),
        ]);
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].name, "a");
    }

    #[test]
    fn source_priority_order() {
        assert!(CommandSource::Builtin.priority() > CommandSource::User.priority());
        assert!(CommandSource::User.priority() > CommandSource::Project.priority());
        assert!(CommandSource::Project.priority() > CommandSource::Plugin.priority());
        assert!(CommandSource::Plugin.priority() > CommandSource::Skill.priority());

        assert_eq!(
            DynamicRegistry::source_priority(CommandSource::Builtin),
            100
        );
        assert_eq!(DynamicRegistry::source_priority(CommandSource::Skill), 20);
    }

    #[test]
    fn list_visible_sorted_by_priority_then_usage() {
        let mut reg = DynamicRegistry::new();
        reg.register(entry("low", CommandSource::Skill, 0.9));
        reg.register(entry("high", CommandSource::Builtin, 0.1));
        reg.register(entry("mid", CommandSource::Plugin, 0.5));

        let visible = reg.list_visible();
        assert_eq!(visible[0].name, "high"); // Builtin
        assert_eq!(visible[1].name, "mid"); // Plugin
        assert_eq!(visible[2].name, "low"); // Skill
    }
}
