# Agentic TUI Color System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Rust TUI 配色从单一角色名/状态色升级为适合多 subagent 的 agentic coding TUI：稳定 agent 身份色、蓝色工具动作语义、常驻上下文信息层。

**Architecture:** 保留现有 `ThemeColors`/`Theme` 兼容字段，新增语义解析层来承载身份、执行、状态、上下文四类 token，避免一次性重命名全量 renderer。Agent 身份色由 role 优先、`agent_id` 稳定 hash 兜底；sticky info 进入独立 context layer，由 `RuntimeViewState` 维护并在 prompt-mode 底部上下文 band 中常驻展示。

**Tech Stack:** Rust 1.91.1, ratatui, crossterm, existing `ThemeProvider`, existing `AgentNavigationState`, existing bottom pane/status rendering.

## Global Constraints

- 只修改 Rust TUI 侧代码：`crates/allthecodes/src/ui/`。
- 保持现有 `Theme` 字段兼容；旧 renderer 仍可继续使用 `assistant_name`、`tool_name`、`info`、`dim` 等字段。
- `assistant_name` 保留为 primary assistant 兼容别名，颜色为紫色 `#BE8CFF` + bold。
- 多 subagent 必须使用稳定身份色；同一个 `agent_id` 在整个 session 中颜色固定。
- Agent 身份色不得使用纯红、纯黄、纯 success 绿；红色留给 error，黄色留给 warning，绿色留给 success/diff add。
- `tool_name`、工具 header、running spinner、`status.info`、`context.info.label` 使用蓝色 `#82C8FF`。
- `info` 可以是蓝色 token，但普通正文不能整段蓝；sticky info 正文使用浅灰/白灰，蓝色只用于 label、icon、左边框或状态 marker。
- `link` 保持 `#64B4FF` + underline，避免与 `tool_name` 的位置语义混淆。
- `selected` 使用黑字 + 蓝底 `#82C8FF` + bold。
- `info.sticky` 常驻 context layer，直到被更新或被明确清除；`info.event` 可留在 timeline，但应可变 dim 或折叠。
- 构建命令必须带项目本地 Rust 环境变量和 `CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target`。
- 不运行仓库内不存在的提交脚本；本计划只创建实现计划，不提交代码。

---

## Current Code Map

| Area | Current file | Current behavior | Planned responsibility |
|------|--------------|------------------|------------------------|
| Theme color table | `crates/allthecodes/src/ui/theme/mod.rs` | `ThemeColors` includes raw color keys and six built-in themes. | Add role/context token colors while keeping existing keys. |
| Legacy render style | `crates/allthecodes/src/ui/rendering/theme.rs` | `Theme` pre-composes styles used by widgets. | Change `tool_name` to blue, expose compatibility aliases and context styles. |
| Color parser | `crates/allthecodes/src/ui/theme/color.rs` | Resolves theme keys, hex, rgb, ansi. | Resolve new semantic token keys. |
| Agent runtime state | `crates/allthecodes/src/ui/app/agent_navigation.rs` | Tracks thread id, nickname, role, status, tool activity. | Supply role/id input to identity color resolver. |
| Agent tree overlay | `crates/allthecodes/src/ui/app/agent_tree_dialog.rs` | Labels use `theme.bold`/`theme.warning`. | Render each agent label with stable identity style. |
| Agent footer | `crates/allthecodes/src/ui/app/render.rs` | Agent footer is dim text plus an info label. | Add current agent and active tool context with identity color. |
| Bottom pane layout | `crates/allthecodes/src/ui/components/bottom_pane.rs` | Already has input, notification, agent footer, status rows. | Add a context row between notification and status or reuse agent footer row when compact. |
| Runtime view state | `crates/allthecodes/src/ui/app/runtime_state.rs` | Holds agent/task runtime display data. | Hold `ContextLayerState`. |
| Tool activity | `crates/allthecodes/src/ui/rendering/tool_activity.rs` | Tool name uses `theme.tool_name`; running uses `theme.info`. | Inherit blue tool name and keep result summaries dim. |
| Command palette/completion | `crates/allthecodes/src/ui/command_palette/render.rs`, `app/render.rs` | Selected is black/white hardcoded. | Move selected blue background into theme in a separate cleanup task. |

---

## Target Token Model

The implementation keeps existing field names and adds semantic accessors so future renderer work can migrate gradually.

```text
identity.assistant.primary
identity.agent.planner
identity.agent.executor
identity.agent.reviewer
identity.agent.tester
identity.agent.researcher
identity.agent.background
identity.agent.hash_pool
identity.user
identity.system

execution.tool.name
execution.tool.args
execution.tool.running
execution.tool.result
execution.command
execution.stdout
execution.stderr

status.info
status.warning
status.error
status.success
status.dim

context.info.border
context.info.label
context.info.text
context.warning
context.error

markdown.heading
markdown.bold
markdown.italic
markdown.link
markdown.code.inline
markdown.code.block

diff.add
diff.remove
diff.context
diff.header

selection.active
selection.inactive
progress.fill
progress.empty
```

Default dark colors:

| Token | Color/effect |
|-------|--------------|
| `identity.assistant.primary` | `#BE8CFF` + bold |
| `identity.agent.planner` | `#8BA7FF` + bold |
| `identity.agent.executor` | `#4FC7B8` + bold |
| `identity.agent.reviewer` | `#FFB15C` + bold |
| `identity.agent.tester` | `#8DE6FF` + bold |
| `identity.agent.researcher` | `#6CBFFF` + bold |
| `identity.agent.background` | `#9A88B8` + bold |
| `execution.tool.name` | `#82C8FF` + bold |
| `execution.tool.args` | dim or inline-code style |
| `status.info` | `#82C8FF` |
| `context.info.label` | `#82C8FF` |
| `context.info.text` | `#DCDCDC` or `surfaceText` |
| `warning` | `#FFC850`, bold only when risk needs emphasis |
| `error` | `#FF6464` + bold |
| `success` | existing success green |
| `dim` | `#646469` |
| `thinking` | dim + italic |
| `code` | yellow-gray text + dark code background |
| `link` | `#64B4FF` + underline |
| `selected` | black text + `#82C8FF` background + bold |
| `context.panel.border` | `#82C8FF` or dimmed blue |
| `context.panel.bg` | slightly brighter dark surface |

---

## Task 1: Correct Core Theme Semantics

**Files:**
- Modify: `crates/allthecodes/src/ui/rendering/theme.rs`
- Modify: `crates/allthecodes/src/ui/theme/mod.rs`
- Modify: `crates/allthecodes/src/ui/theme/color.rs`

**Interfaces:**
- Consumes: Existing `ThemeColors` and `Theme::from_design_colors(colors: &ThemeColors) -> Theme`.
- Produces: Blue `Theme::tool_name`, blue `Theme::info`, blue `Theme::selected` background, and context style fields used by later tasks.

- [ ] **Step 1: Add failing tests for blue tool and context styles**

Add these tests to `crates/allthecodes/src/ui/theme/mod.rs` test module:

```rust
#[test]
fn dark_theme_uses_blue_tool_name_and_info() {
    let provider = ThemeProvider::with_name(ThemeName::Dark);
    let colors = provider.colors();
    let legacy = provider.legacy_theme();

    assert_eq!(colors.info, ratatui::style::Color::Rgb(130, 200, 255));
    assert_eq!(legacy.info.fg, Some(ratatui::style::Color::Rgb(130, 200, 255)));
    assert_eq!(legacy.tool_name.fg, Some(ratatui::style::Color::Rgb(130, 200, 255)));
    assert!(legacy.tool_name.add_modifier.contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn dark_theme_context_info_body_is_not_blue() {
    let provider = ThemeProvider::with_name(ThemeName::Dark);
    let legacy = provider.legacy_theme();

    assert_eq!(legacy.context_info_label.fg, Some(ratatui::style::Color::Rgb(130, 200, 255)));
    assert_ne!(legacy.context_info_text.fg, Some(ratatui::style::Color::Rgb(130, 200, 255)));
}
```

- [ ] **Step 2: Run tests and confirm they fail before implementation**

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo test -p allthecodes theme::tests::dark_theme_uses_blue_tool_name_and_info theme::tests::dark_theme_context_info_body_is_not_blue
```

Expected: fails because `context_info_label`/`context_info_text` do not exist and `tool_name` is currently code-yellow.

- [ ] **Step 3: Extend `Theme` with context styles**

Add these fields to `Theme` in `crates/allthecodes/src/ui/rendering/theme.rs`:

```rust
    /// Sticky context info label/icon/border style.
    pub context_info_label: Style,
    /// Sticky context info body style; intentionally not blue.
    pub context_info_text: Style,
    /// Sticky context warning style.
    pub context_warning: Style,
    /// Sticky context error style.
    pub context_error: Style,
```

Add matching defaults in `impl Default for Theme`:

```rust
            context_info_label: Style::default().fg(Color::Rgb(130, 200, 255)),
            context_info_text: Style::default().fg(Color::Rgb(220, 220, 220)),
            context_warning: Style::default().fg(Color::Rgb(255, 200, 80)),
            context_error: Style::default()
                .fg(Color::Rgb(255, 100, 100))
                .add_modifier(Modifier::BOLD),
```

- [ ] **Step 4: Change tool styling in design theme conversion**

In `Theme::from_design_colors` inside `crates/allthecodes/src/ui/theme/mod.rs`, change `tool_name` from code color to info blue:

```rust
                    tool_name: Style::default()
                        .fg(theme_color("info", colors.info))
                        .add_modifier(Modifier::BOLD),
```

Add the new context fields in the same `build_theme!` initializer:

```rust
                    context_info_label: Style::default().fg(colors.info),
                    context_info_text: Style::default().fg(colors.surfaceText),
                    context_warning: Style::default().fg(colors.warning),
                    context_error: Style::default()
                        .fg(colors.error)
                        .add_modifier(Modifier::BOLD),
```

In `Theme::default()` in `rendering/theme.rs`, change `tool_name` to:

```rust
            tool_name: Style::default()
                .fg(Color::Rgb(130, 200, 255))
                .add_modifier(Modifier::BOLD),
```

- [ ] **Step 5: Add color resolver keys for context tokens**

In `resolve_theme_key` in `crates/allthecodes/src/ui/theme/color.rs`, add normalized key mappings:

```rust
        "contextinfoborder" => Some(colors.info),
        "contextinfolabel" => Some(colors.info),
        "contextinfotext" => Some(colors.surfaceText),
        "contextwarning" => Some(colors.warning),
        "contexterror" => Some(colors.error),
```

Add a test:

```rust
#[test]
fn resolve_context_info_keys() {
    let c = dark_colors();
    assert_eq!(resolve_color("context.info.label", c), None);
    assert_eq!(resolve_color("context-info-label", c), Some(c.info));
    assert_eq!(resolve_color("context_info_text", c), Some(c.surfaceText));
}
```

The dotted form intentionally remains unsupported until the parser is expanded; use hyphen/underscore names in Rust code.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p allthecodes theme::tests::dark_theme_uses_blue_tool_name_and_info theme::tests::dark_theme_context_info_body_is_not_blue
cargo test -p allthecodes theme::color::tests::resolve_context_info_keys
```

Expected: all selected tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/rendering/theme.rs crates/allthecodes/src/ui/theme/mod.rs crates/allthecodes/src/ui/theme/color.rs
git commit -m "Update TUI theme semantics"
```

---

## Task 2: Add Agent Identity Palette

**Files:**
- Create: `crates/allthecodes/src/ui/theme/identity.rs`
- Modify: `crates/allthecodes/src/ui/theme/mod.rs`
- Modify: `crates/allthecodes/src/ui/theme/color.rs`
- Modify: `crates/allthecodes/src/ui/mod.rs` if module re-export wiring requires it.

**Interfaces:**
- Consumes: `ThemeColors`, `agent_id`, optional `agent_role`, `is_primary`.
- Produces:

```rust
pub enum AgentIdentityRole;
pub struct AgentIdentity<'a>;
pub fn agent_identity_style(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Style;
pub fn agent_identity_color(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Color;
```

- [ ] **Step 1: Write failing tests for role and hash stability**

Create `crates/allthecodes/src/ui/theme/identity.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};
    use ratatui::style::{Color, Modifier};

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[test]
    fn primary_agent_keeps_assistant_purple() {
        let style = agent_identity_style(
            dark(),
            AgentIdentity {
                agent_id: "primary",
                role: Some("main"),
                is_primary: true,
            },
        );
        assert_eq!(style.fg, Some(Color::Rgb(190, 140, 255)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn known_roles_use_reserved_safe_identity_colours() {
        let planner = agent_identity_color(dark(), AgentIdentity {
            agent_id: "agent-planner",
            role: Some("planner"),
            is_primary: false,
        });
        let executor = agent_identity_color(dark(), AgentIdentity {
            agent_id: "agent-executor",
            role: Some("executor"),
            is_primary: false,
        });
        let reviewer = agent_identity_color(dark(), AgentIdentity {
            agent_id: "agent-reviewer",
            role: Some("reviewer"),
            is_primary: false,
        });

        assert_eq!(planner, dark().agentPlanner);
        assert_eq!(executor, dark().agentExecutor);
        assert_eq!(reviewer, dark().agentReviewer);
        assert_ne!(reviewer, dark().warning);
        assert_ne!(executor, dark().success);
    }

    #[test]
    fn unknown_agent_hash_is_stable() {
        let first = agent_identity_color(dark(), AgentIdentity {
            agent_id: "worker-thread-123",
            role: Some("unknown"),
            is_primary: false,
        });
        let second = agent_identity_color(dark(), AgentIdentity {
            agent_id: "worker-thread-123",
            role: None,
            is_primary: false,
        });
        assert_eq!(first, second);
    }
}
```

- [ ] **Step 2: Run tests and confirm missing symbols**

Run:

```bash
cargo test -p allthecodes theme::identity
```

Expected: fails until the module is wired and fields/functions exist.

- [ ] **Step 3: Add role color fields to `ThemeColors`**

In `crates/allthecodes/src/ui/theme/mod.rs`, extend `ThemeColors` after existing agent colors:

```rust
    // -- Agent identity roles --
    pub agentPlanner: Color,
    pub agentExecutor: Color,
    pub agentReviewer: Color,
    pub agentTester: Color,
    pub agentResearcher: Color,
    pub agentBackground: Color,
```

Add dark theme values:

```rust
        agentPlanner: Color::Rgb(139, 167, 255),
        agentExecutor: Color::Rgb(79, 199, 184),
        agentReviewer: Color::Rgb(255, 177, 92),
        agentTester: Color::Rgb(141, 230, 255),
        agentResearcher: Color::Rgb(108, 191, 255),
        agentBackground: Color::Rgb(154, 136, 184),
```

Add light theme values with stronger contrast:

```rust
        agentPlanner: Color::Rgb(70, 95, 190),
        agentExecutor: Color::Rgb(20, 130, 125),
        agentReviewer: Color::Rgb(170, 95, 25),
        agentTester: Color::Rgb(20, 130, 170),
        agentResearcher: Color::Rgb(35, 105, 190),
        agentBackground: Color::Rgb(120, 105, 145),
```

For daltonized themes, override `agentExecutor` away from success-like green:

```rust
    c.agentExecutor = Color::Rgb(50, 150, 180);
    c.agentReviewer = Color::Rgb(210, 120, 40);
```

For ANSI themes, keep these role colors as RGB instead of terminal indexed colors so identities remain distinguishable from status colors.

- [ ] **Step 4: Implement identity module**

Use deterministic FNV-1a hashing rather than `DefaultHasher`, because identity color assignment must not depend on randomized process state:

```rust
use ratatui::style::{Color, Modifier, Style};

use super::ThemeColors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIdentityRole {
    Primary,
    Planner,
    Executor,
    Reviewer,
    Tester,
    Researcher,
    Background,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentIdentity<'a> {
    pub agent_id: &'a str,
    pub role: Option<&'a str>,
    pub is_primary: bool,
}

pub fn role_from_agent_role(role: Option<&str>, is_primary: bool) -> AgentIdentityRole {
    if is_primary {
        return AgentIdentityRole::Primary;
    }
    let normalized = role.unwrap_or("").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "planner" | "plan" | "architect" => AgentIdentityRole::Planner,
        "executor" | "builder" | "worker" | "implementer" => AgentIdentityRole::Executor,
        "reviewer" | "review" | "critic" => AgentIdentityRole::Reviewer,
        "tester" | "test" | "qa" | "verifier" => AgentIdentityRole::Tester,
        "researcher" | "research" | "explorer" | "investigator" => AgentIdentityRole::Researcher,
        "background" | "daemon" | "monitor" => AgentIdentityRole::Background,
        _ => AgentIdentityRole::Unknown,
    }
}

pub fn agent_identity_color(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Color {
    match role_from_agent_role(identity.role, identity.is_primary) {
        AgentIdentityRole::Primary => colors.accent,
        AgentIdentityRole::Planner => colors.agentPlanner,
        AgentIdentityRole::Executor => colors.agentExecutor,
        AgentIdentityRole::Reviewer => colors.agentReviewer,
        AgentIdentityRole::Tester => colors.agentTester,
        AgentIdentityRole::Researcher => colors.agentResearcher,
        AgentIdentityRole::Background => colors.agentBackground,
        AgentIdentityRole::Unknown => {
            let palette = [
                colors.agentBlue,
                colors.agentCyan,
                colors.agentPurple,
                colors.agentPink,
                colors.agentOrange,
                colors.agentBackground,
            ];
            let index = (fnv1a64(identity.agent_id.as_bytes()) as usize) % palette.len();
            palette[index]
        }
    }
}

pub fn agent_identity_style(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Style {
    Style::default()
        .fg(agent_identity_color(colors, identity))
        .add_modifier(Modifier::BOLD)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
```

Wire it in `theme/mod.rs`:

```rust
pub mod color;
pub mod identity;
```

- [ ] **Step 5: Add resolver keys for role colors**

In `theme/color.rs`, add:

```rust
        "agentplanner" => Some(colors.agentPlanner),
        "agentexecutor" => Some(colors.agentExecutor),
        "agentreviewer" => Some(colors.agentReviewer),
        "agenttester" => Some(colors.agentTester),
        "agentresearcher" => Some(colors.agentResearcher),
        "agentbackground" => Some(colors.agentBackground),
```

Add a resolver test:

```rust
#[test]
fn resolve_agent_identity_role_keys() {
    let c = dark_colors();
    assert_eq!(resolve_color("agent-planner", c), Some(c.agentPlanner));
    assert_eq!(resolve_color("agent_executor", c), Some(c.agentExecutor));
}
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p allthecodes theme::identity
cargo test -p allthecodes theme::color::tests::resolve_agent_identity_role_keys
```

Expected: all selected tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/theme/mod.rs crates/allthecodes/src/ui/theme/color.rs crates/allthecodes/src/ui/theme/identity.rs
git commit -m "Add TUI agent identity palette"
```

---

## Task 3: Apply Agent Identity Colors to Agent Surfaces

**Files:**
- Modify: `crates/allthecodes/src/ui/app/agent_tree_dialog.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/runtime_state.rs` only if helper accessors are needed.

**Interfaces:**
- Consumes: `AgentNavigationState::ordered_threads()`, `AgentThreadEntry`.
- Produces: Styled agent labels in agent tree and footer using `agent_identity_style`.

- [ ] **Step 1: Write failing agent tree style test**

In `agent_tree_dialog.rs` tests, add a buffer/style-level assertion:

```rust
#[test]
fn agent_tree_uses_role_identity_colours() {
    use crate::ui::app::agent_navigation::{AgentNavigationState, AgentThreadEntry};
    use crate::ui::theme::{get_theme, Theme, ThemeName};

    let mut state = AgentNavigationState::default();
    state.upsert(AgentThreadEntry {
        thread_id: "planner-1".to_string(),
        agent_nickname: Some("Plan agent".to_string()),
        agent_role: Some("planner".to_string()),
        is_primary: false,
        is_closed: false,
    });

    let mut dialog = AgentTreeDialog::from_state(&state, "planner-1");
    let lines = dialog.render_lines(&state, "planner-1", &Theme::default(), get_theme(&ThemeName::Dark));
    let label_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref().contains("Plan agent"))
        .expect("planner label span");

    assert_eq!(label_span.style.fg, Some(get_theme(&ThemeName::Dark).agentPlanner));
}
```

- [ ] **Step 2: Change `AgentTreeDialog::render_lines` signature**

Update signature:

```rust
pub fn render_lines(
    &mut self,
    state: &AgentNavigationState,
    current_thread_id: &str,
    theme: &Theme,
    colors: &crate::ui::theme::ThemeColors,
) -> Vec<Line<'static>>
```

Replace label style logic:

```rust
let mut label_style = if entry.is_closed {
    theme.warning
} else {
    crate::ui::theme::identity::agent_identity_style(
        colors,
        crate::ui::theme::identity::AgentIdentity {
            agent_id: &entry.thread_id,
            role: entry.agent_role.as_deref(),
            is_primary: entry.is_primary,
        },
    )
};
if is_current {
    label_style = label_style.add_modifier(Modifier::BOLD);
}
```

- [ ] **Step 3: Update app overlay call sites**

In `render_agent_tree_overlay` in `app/render.rs`, call:

```rust
let mut lines = dialog.render_lines(state, current_thread_id, theme, colors);
```

Update any tests that call `render_lines` directly to pass `get_theme(&ThemeName::Dark)` or the active `ThemeColors`.

- [ ] **Step 4: Add styled agent footer entries**

In `app.rs`, add a small value type near existing agent footer helpers:

```rust
pub(super) struct AgentFooterEntry {
    pub thread_id: String,
    pub label: String,
    pub role: Option<String>,
    pub is_primary: bool,
    pub status: String,
}
```

Add:

```rust
pub(super) fn agent_footer_entries(&self) -> Vec<AgentFooterEntry> {
    self.runtime_view
        .agent_nav()
        .ordered_threads()
        .into_iter()
        .filter(|entry| !entry.is_primary && !entry.is_closed)
        .map(|entry| {
            let status = self.runtime_view
                .agent_nav()
                .runtime_info(&entry.thread_id)
                .map(|runtime| runtime.status.label())
                .unwrap_or("active")
                .to_string();
            AgentFooterEntry {
                thread_id: entry.thread_id.clone(),
                label: entry.label(),
                role: entry.agent_role.clone(),
                is_primary: entry.is_primary,
                status,
            }
        })
        .collect()
}
```

In `render_agent_footer`, prefer `agent_footer_entries()` and style each label with `agent_identity_style`. The line shape should remain compact:

```text
 agents  Plan agent[thinking]  Build worker[tool]
```

Only labels get identity color; brackets/status stay dim or status-colored.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p allthecodes agent_tree_dialog
cargo test -p allthecodes app::runtime_state::tests::runtime_view_state_tracks_current_agent_thread
```

Expected: tests pass and snapshots that include agent tree are updated only when color/style metadata is asserted.

- [ ] **Step 6: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/app/agent_tree_dialog.rs crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/runtime_state.rs
git commit -m "Apply agent identity colors in TUI"
```

---

## Task 4: Add Sticky Context Layer State and Renderer

**Files:**
- Create: `crates/allthecodes/src/ui/context_layer.rs`
- Modify: `crates/allthecodes/src/ui/mod.rs`
- Modify: `crates/allthecodes/src/ui/app/runtime_state.rs`
- Modify: `crates/allthecodes/src/ui/components/bottom_pane.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`

**Interfaces:**
- Produces:

```rust
pub enum ContextTone;
pub struct ContextLayerItem;
pub struct ContextLayerState;
pub fn render_context_layer(items: &[ContextLayerItem], width: usize, theme: &Theme) -> Vec<Line<'static>>;
```

- [ ] **Step 1: Write renderer tests first**

Create `context_layer.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    #[test]
    fn context_info_label_is_blue_but_body_is_not() {
        let theme = Theme::default();
        let lines = render_context_layer(
            &[ContextLayerItem::new(ContextTone::Info, "branch", "feature/agent-loop")],
            80,
            &theme,
        );
        let label = lines[0].spans.iter().find(|span| span.content.as_ref().contains("branch")).unwrap();
        let body = lines[0].spans.iter().find(|span| span.content.as_ref().contains("feature/agent-loop")).unwrap();

        assert_eq!(label.style.fg, theme.context_info_label.fg);
        assert_eq!(body.style.fg, theme.context_info_text.fg);
        assert_ne!(body.style.fg, theme.context_info_label.fg);
    }

    #[test]
    fn context_layer_truncates_to_width() {
        let theme = Theme::default();
        let lines = render_context_layer(
            &[ContextLayerItem::new(ContextTone::Info, "repo", "a-very-long-repository-name-that-must-fit")],
            24,
            &theme,
        );
        assert!(lines[0].width() <= 24);
    }
}
```

- [ ] **Step 2: Implement context layer types**

Use this structure:

```rust
use std::collections::BTreeMap;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextTone {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextLayerKey {
    Repo,
    Branch,
    Plan,
    CurrentAgent,
    CurrentTool,
    ContextUsage,
    PendingPermission,
    LastError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLayerItem {
    pub key: ContextLayerKey,
    pub tone: ContextTone,
    pub label: String,
    pub value: String,
}

impl ContextLayerItem {
    pub fn new(tone: ContextTone, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: ContextLayerKey::Repo,
            tone,
            label: label.into(),
            value: value.into(),
        }
    }

    pub fn keyed(
        key: ContextLayerKey,
        tone: ContextTone,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key,
            tone,
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ContextLayerState {
    items: BTreeMap<ContextLayerKey, ContextLayerItem>,
}

impl ContextLayerState {
    pub fn upsert(&mut self, item: ContextLayerItem) {
        self.items.insert(item.key.clone(), item);
    }

    pub fn remove(&mut self, key: &ContextLayerKey) {
        self.items.remove(key);
    }

    pub fn items(&self) -> Vec<ContextLayerItem> {
        self.items.values().cloned().collect()
    }
}
```

Implement rendering:

```rust
pub fn render_context_layer(
    items: &[ContextLayerItem],
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if width == 0 || items.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::new();
    spans.push(Span::styled(" info ", theme.context_info_label));
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" | ", theme.dim));
        }
        spans.push(Span::styled(format!("{}: ", item.label), style_for_label(item.tone, theme)));
        spans.push(Span::styled(item.value.clone(), style_for_body(item.tone, theme)));
    }
    vec![Line::from(truncate_spans(spans, width))]
}

fn style_for_label(tone: ContextTone, theme: &Theme) -> Style {
    match tone {
        ContextTone::Info => theme.context_info_label,
        ContextTone::Warning => theme.context_warning,
        ContextTone::Error => theme.context_error,
    }
}

fn style_for_body(tone: ContextTone, theme: &Theme) -> Style {
    match tone {
        ContextTone::Info => theme.context_info_text,
        ContextTone::Warning => theme.context_info_text,
        ContextTone::Error => theme.context_info_text,
    }
}
```

`truncate_spans` should preserve span styles while limiting total displayed width. Use the same width logic style as `diff/structured_diff.rs` rather than byte truncation.

- [ ] **Step 3: Wire module and runtime state**

In `crates/allthecodes/src/ui/mod.rs`:

```rust
#[path = "context_layer.rs"]
pub mod context_layer;
```

In `RuntimeViewState` add:

```rust
    context_layer: crate::ui::context_layer::ContextLayerState,
```

Initialize in `Default`:

```rust
            context_layer: crate::ui::context_layer::ContextLayerState::default(),
```

Add accessors:

```rust
pub(super) fn context_layer(&self) -> &crate::ui::context_layer::ContextLayerState {
    &self.context_layer
}

pub(super) fn context_layer_mut(&mut self) -> &mut crate::ui::context_layer::ContextLayerState {
    &mut self.context_layer
}
```

- [ ] **Step 4: Add bottom pane context row**

In `BottomPaneHeights`, add:

```rust
    pub context: u16,
```

Place it after `notification` and before `agent_footer`:

```rust
            Constraint::Length(self.notification),
            Constraint::Length(self.context),
            Constraint::Length(self.agent_footer),
            Constraint::Length(self.status),
```

Update `BottomPaneAreas`:

```rust
    pub context: Rect,
```

Update `total()` and `split()` mapping. The test `height_model_splits_terminal_regions` should assert:

```rust
assert!(areas.context.y > areas.input.y);
assert!(areas.context.y < areas.status.y);
```

- [ ] **Step 5: Render context row**

In `app/render.rs`, compute:

```rust
let context_height = u16::from(!self.runtime_view.context_layer().items().is_empty());
```

Set in `BottomPaneHeights`:

```rust
context: context_height,
```

Add renderer:

```rust
fn render_context_layer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    if area.height == 0 {
        return;
    }
    let items = self.runtime_view.context_layer().items();
    let lines = crate::ui::context_layer::render_context_layer(
        &items,
        area.width as usize,
        &self.theme,
    );
    if let Some(line) = lines.first() {
        buf.set_line(area.x, area.y, line, area.width);
    }
}
```

Call it after notification and before agent footer:

```rust
if context_height > 0 {
    self.render_context_layer(bottom_chunks.context, frame.buffer_mut());
}
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p allthecodes context_layer
cargo test -p allthecodes bottom_pane
cargo test -p allthecodes app::runtime_state
```

Expected: context renderer, bottom pane split, and runtime state tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/context_layer.rs crates/allthecodes/src/ui/mod.rs crates/allthecodes/src/ui/app/runtime_state.rs crates/allthecodes/src/ui/components/bottom_pane.rs crates/allthecodes/src/ui/app/render.rs
git commit -m "Add sticky TUI context layer"
```

---

## Task 5: Populate Sticky Context Items

**Files:**
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/app/runtime_state.rs`
- Modify: `crates/allthecodes/src/ui/tui/subsystem_events.rs` only if system info events need direct mapping.

**Interfaces:**
- Consumes: repo/cwd data from `session_ui`, model name, active goal, `current_agent_thread_id`, `AgentNavigationState`, running tool activity summaries, pending permission state.
- Produces: Persistent context items:
  - repo/cwd
  - branch when available
  - plan/goal status
  - current agent
  - current running tool
  - context usage when known
  - pending permission
  - latest error

- [ ] **Step 1: Add context item builder test**

In `app.rs` tests, add a focused test around an app with a current agent:

```rust
#[test]
fn app_context_layer_includes_current_agent() {
    let mut app = App::new();
    app.runtime_view.upsert_agent(crate::ui::app::agent_navigation::AgentThreadEntry {
        thread_id: "worker-1".to_string(),
        agent_nickname: Some("Build worker".to_string()),
        agent_role: Some("executor".to_string()),
        is_primary: false,
        is_closed: false,
    });
    app.runtime_view.set_current_agent_thread(Some("worker-1".to_string()));

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();

    assert!(items.iter().any(|item| item.label == "agent" && item.value.contains("Build worker")));
}
```

- [ ] **Step 2: Implement `refresh_context_layer`**

Add this method in `impl App`:

```rust
pub(super) fn refresh_context_layer(&mut self) {
    use crate::ui::context_layer::{ContextLayerItem, ContextLayerKey, ContextTone};

    let context = self.runtime_view.context_layer_mut();

    if !self.session_ui.cwd.is_empty() {
        context.upsert(ContextLayerItem::keyed(
            ContextLayerKey::Repo,
            ContextTone::Info,
            "repo",
            self.session_ui.cwd.clone(),
        ));
    }

    if let Some(goal) = &self.active_goal {
        context.upsert(ContextLayerItem::keyed(
            ContextLayerKey::Plan,
            ContextTone::Info,
            "plan",
            format!(
                "{} {}",
                render_goal_status(&goal.status),
                truncate_status_text(&goal.objective, 28)
            ),
        ));
    }

    if let Some(thread_id) = self.runtime_view.current_agent_thread_id().cloned() {
        if let Some(entry) = self.runtime_view.agent_nav().entry(&thread_id) {
            context.upsert(ContextLayerItem::keyed(
                ContextLayerKey::CurrentAgent,
                ContextTone::Info,
                "agent",
                entry.label(),
            ));
        }
    }
}
```

Call `self.refresh_context_layer()` once per frame before bottom pane height calculation in `render()`, after runtime state has been updated and before `context_height` is computed.

- [ ] **Step 3: Add running tool and permission context**

In `refresh_context_layer`, add current running tool:

```rust
if let Some(thread_id) = self.runtime_view.current_agent_thread_id().cloned() {
    if let Some(runtime) = self.runtime_view.agent_nav().runtime_info(&thread_id) {
        if let Some(tool) = runtime.recent_tool_uses().last() {
            context.upsert(ContextLayerItem::keyed(
                ContextLayerKey::CurrentTool,
                ContextTone::Info,
                "tool",
                format!("{} {}", tool.tool_name, tool.summary),
            ));
        }
    }
}
```

When a permission overlay is active, upsert:

```rust
context.upsert(ContextLayerItem::keyed(
    ContextLayerKey::PendingPermission,
    ContextTone::Warning,
    "permission",
    "pending approval",
));
```

When no permission overlay is active, remove `PendingPermission`.

- [ ] **Step 4: Keep latest error sticky but bounded**

When a notification tone is error or an agent status becomes failed, upsert:

```rust
context.upsert(ContextLayerItem::keyed(
    ContextLayerKey::LastError,
    ContextTone::Error,
    "error",
    compact_inline(error_text, 96),
));
```

Do not auto-clear `LastError` during ordinary render. Clear it only when a new successful user turn starts or when a higher-level error clear event exists.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p allthecodes app_context_layer_includes_current_agent
cargo test -p allthecodes context_layer
```

Expected: context items include current agent and render with label/body style split.

- [ ] **Step 6: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/app/runtime_state.rs crates/allthecodes/src/ui/tui/subsystem_events.rs
git commit -m "Populate sticky TUI context info"
```

---

## Task 6: Split Info Event vs Sticky Info Presentation

**Files:**
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/notifications/in_app.rs`
- Modify: `crates/allthecodes/src/ui/tui/subsystem_events.rs`
- Modify: `crates/allthecodes/src/ui/messages/render/render_assistant.rs` only if info messages are currently rendered in the timeline with full blue text.

**Interfaces:**
- Consumes: `NotificationTone`, `InfoLevel`, existing notification system.
- Produces: Explicit distinction between `info.event` and `info.sticky`.

- [ ] **Step 1: Add notification style regression test**

In `app/render.rs` tests or a new focused notification test, assert tone mapping:

```rust
#[test]
fn notification_info_uses_blue_but_body_can_move_to_context_layer() {
    let theme = Theme::default();
    assert_eq!(notification_style(NotificationTone::Info, &theme).fg, theme.info.fg);
    assert_eq!(notification_style(NotificationTone::Dim, &theme).fg, theme.dim.fg);
}
```

- [ ] **Step 2: Keep transient notification blue but compact**

Do not make whole sticky context body blue. In `render_notification`, leave:

```rust
Span::styled(" notice ", self.theme.info)
```

but ensure `notification.text` uses `notification_style(...)`, not `context_info_label`.

- [ ] **Step 3: Route persistent system info to context layer**

For `InfoLevel::Info` subsystem messages that represent current state rather than historical events, update `subsystem_events.rs` to upsert `ContextLayerKey` through an app event path. Use timeline notification only for event text that users should see once.

Rule:

| Incoming info | Surface |
|---------------|---------|
| current branch/repo/model/context usage | sticky context layer |
| ordinary notification text | transient notification row |
| warning/error | transient notification plus sticky warning/error when actionable |

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p allthecodes notification_info_uses_blue_but_body_can_move_to_context_layer
cargo test -p allthecodes subsystem_events
```

Expected: existing subsystem event tests still pass; new presentation test passes.

- [ ] **Step 5: Commit**

```bash
git add -A -- crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/notifications/in_app.rs crates/allthecodes/src/ui/tui/subsystem_events.rs
git commit -m "Separate sticky and event info styles"
```

---

## Task 7: Normalize Selection and Completion Blue

**Batch 4 status (2026-07-09):** Implemented. Command palette and completion
popup selected rows now use `theme.selected`; cursor and unrelated permission UI
were left unchanged.

**Files:**
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/command_palette/render.rs`
- Modify: `crates/allthecodes/src/ui/prompt_input.rs` only if cursor/selection style should use theme-selected.

**Interfaces:**
- Consumes: `theme.selected`.
- Produces: selected rows use black text + `#82C8FF` background + bold instead of hardcoded black/white.

- [x] **Step 1: Add selection style tests**

In command palette rendering tests, assert selected command cell uses `theme.selected`:

```rust
#[test]
fn command_palette_selected_row_uses_theme_selected() {
    let mut palette = CommandPalette::default();
    palette.open();
    palette.update_query("/");

    let area = Rect::new(0, 0, 100, palette.preferred_height());
    let mut buf = Buffer::empty(area);
    let theme = Theme::default();
    palette.render(area, &mut buf, &theme);

    let selected_cell = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .map(|(x, y)| &buf[(x, y)])
        .find(|cell| cell.style().bg == theme.selected.bg)
        .expect("selected cell with theme background");
    assert_eq!(selected_cell.style().fg, theme.selected.fg);
}
```

- [x] **Step 2: Replace hardcoded black/white selected styles**

In `command_palette/render.rs`, replace:

```rust
Style::default()
    .fg(Color::Black)
    .bg(Color::White)
    .add_modifier(Modifier::BOLD)
```

with:

```rust
theme.selected
```

In completion popup rendering in `app/render.rs`, replace selected item style with `self.theme.selected`. Keep the green/darkgray arrow marker for now, because it communicates selection cursor separately from row fill.

- [x] **Step 3: Run focused tests**

Run:

```bash
cargo test -p allthecodes command_palette
cargo test -p allthecodes completion_popup
```

Expected: selected rows use theme-selected style and existing command palette snapshots are updated only for style metadata or equivalent visual output.

- [ ] **Step 4: Commit** _(skipped for Batch 4: user requested no commit)_

```bash
git add -A -- crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/command_palette/render.rs crates/allthecodes/src/ui/prompt_input.rs
git commit -m "Use blue theme selection in TUI"
```

---

## Task 8: Documentation and Verification

**Batch 4 status (2026-07-09):** Final verification completed with
`cargo fmt --all --check`, focused TUI tests, `cargo check -p allthecodes`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo build --workspace --release`. No `docs/KNOWN_ISSUES.md` update was needed.

**Files:**
- Modify: `docs/KNOWN_ISSUES.md` only if implementation uncovers a user-visible TUI behavior gap.
- Modify: `development/tui/2026-07-09-agentic-tui-color-context-layer-plan.md` status section after implementation.

**Interfaces:**
- Produces: verified implementation with focused tests, format, clippy, and release build.

- [x] **Step 1: Run formatting**

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo fmt --all --check
```

Expected: exit 0. If it fails, run `cargo fmt --all`, then rerun `cargo fmt --all --check`.

- [x] **Step 2: Run focused TUI tests**

Run:

```bash
cargo test -p allthecodes theme::identity
cargo test -p allthecodes context_layer
cargo test -p allthecodes agent_tree_dialog
cargo test -p allthecodes command_palette
cargo test -p allthecodes bottom_pane
```

Expected: all exit 0.

- [x] **Step 3: Run clippy**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: exit 0. Fix any warning in touched files instead of allowing or suppressing it without a narrow reason.

- [x] **Step 4: Run release build**

Run:

```bash
cargo build --workspace --release
```

Expected: exit 0.

- [ ] **Step 5: Commit final documentation status if needed** _(skipped for Batch 4: user requested no commit)_

If implementation updates this plan status or `docs/KNOWN_ISSUES.md`, commit only those doc paths:

```bash
git add -A -- development/tui/2026-07-09-agentic-tui-color-context-layer-plan.md docs/KNOWN_ISSUES.md
git commit -m "Document TUI color context status"
```

---

## Self-Review Checklist

- [x] `assistant_name` remains primary assistant purple `#BE8CFF` + bold.
- [x] Multi-agent identity color is role-first and stable-hash fallback by `agent_id`.
- [x] Agent colors avoid pure red, pure warning yellow, and pure success green.
- [x] `tool_name` is blue `#82C8FF` + bold.
- [x] `info` label is blue `#82C8FF`; sticky info body is not all-blue.
- [x] Sticky context layer includes current repo/cwd, model, plan/goal, current agent, current tool, permission, and latest error where data exists.
- [x] Transient notification info remains event-like and does not become sticky by default.
- [x] Selected rows use black text + blue background + bold via `theme.selected`.
- [x] Tests cover theme, identity palette, context renderer, agent tree, bottom pane, command palette, completion popup, clippy, and release build.
