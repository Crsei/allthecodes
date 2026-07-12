# Allthecodes TUI Logo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Rust TUI 的紧凑欢迎面板中加入一个固定 3×3 九宫格 logo，以可测试的离散动画依次显示 `ALLTHECODES` 的 11 个字母，同时保留现有会话信息、输入框位置和小终端降级行为。

**Architecture:** 新建纯粹的 `brand_logo` 组件，分别承载 3×3 字形表、80ms 动画状态机和 ratatui 渲染；`welcome` 只负责响应式排版，`App` 只负责在欢迎页真实可见时推进状态并触发 dirty redraw。设计借鉴 Codex TUI 的“独立动画控制器 + widget 尺寸门槛 + 固定帧测试”，但使用本项目九宫格品牌资产和现有 16ms tick，不复制 Codex 的 36 帧字符资源。

**Tech Stack:** Rust 1.91.1, ratatui, crossterm, insta snapshots, existing `AppEvent::Tick`, existing dirty-render loop.

## Global Constraints

- 生产代码只修改 `crates/allthecodes/src/ui/`；不改非 Rust TUI，也不引入新的 crate dependency。
- 按 Full Build 实现完整的尺寸、主题、重复字母、暂停和降级分支，不以历史 Lite 行为为边界。
- 动画序列必须精确为 `A L L T H E C O D E S`；两个连续的 `L` 必须保留为两个不同索引，不能去重。
- logo 只在 `show_welcome == true` 且欢迎面板实际使用宽屏布局时动画；首个 user/assistant message、initial prompt、workspace trust gate、transcript/focus view 或窄屏布局均不得产生无意义 redraw。
- 欢迎面板继续保持 `PANEL_HEIGHT = 8`，输入框前的空白行和 prompt 行位置不变。
- `icon.png` 只作为视觉依据，不作为运行时资源，不从 sibling repository 读取、复制或 `include_bytes!`：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-app/assets/icon.png`。
- truecolor 使用图标采样的蓝→cyan→teal 品牌色；ANSI theme 使用命名色；颜色被终端忽略时仍必须靠 `░░`、`▒▒`、`▓▓`、`██` 密度辨认状态。
- 每个任务开始和提交前都运行 `git status --short`；除本计划列出的显式实现路径外，所有当时存在的 dirty/untracked 路径都视为用户改动，不得覆盖或暂存。不要把某一次 preflight 的瞬时路径清单写进提交命令。
- PTY suite 已由基线 commit `7cbffe52` 纳入版本控制；本任务只修改其中的 `crates/allthecodes/tests/pty_tui_e2e/welcome.rs`，其余 PTY 文件保持不变。
- 所有 Cargo 命令使用：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

- 每次 commit 只显式暂存本任务路径；文档改动与实现代码分开提交。

---

## Reference Baseline and Intentional Differences

Codex 参考基线：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex` commit `e9641ad5122539bbdfea42eb1faabca7e8184e29`。

| Reference | Confirmed behavior | Reused here |
|---|---|---|
| `codex-rs/tui/src/frames.rs` | 编译期嵌入 10×36 个字符帧，默认 80ms | 使用 80ms 视觉帧，但字形改为代码内 3×3 masks |
| `codex-rs/tui/src/ascii_animation.rs` | 动画控制与 widget 分离，按时间选帧 | 独立 `WelcomeLogoState`，render 接受固定 `LogoFrame` |
| `codex-rs/tui/src/onboarding/welcome.rs` | 尺寸不足时隐藏图案；Buffer 单测固定布局 | 宽屏显示、窄屏完整降级、确定性 Buffer/snapshot 测试 |
| `codex-rs/tui/src/history_cell/session.rs` | 正常会话使用紧凑静态品牌 header | 保留本项目现有 8 行紧凑欢迎面板和详情 |

以下差异是 **Intentional**，实现和 PR 描述必须明确保留：

- 本项目 logo 位于正常 idle welcome，而不是 Codex 的登录 onboarding，因为当前 Rust TUI 没有对应的 onboarding widget 树。
- 不移植 Codex 的 38×17 大图、随机 variant 或 `Ctrl+.` 切换；品牌效果固定为 `ALLTHECODES` 九宫格序列。
- logo 因窄屏隐藏时停止 dirty redraw；这有意修正 Codex“小屏仍调度下一帧”的额外开销。
- 本任务不新增只控制 logo 的全局 `animations` 配置；若以后补齐 motion preference，应统一覆盖 spinner、thinking、shimmer 和 welcome logo，而不是形成孤立开关。
- 图标使用主题感知颜色和字符密度 fallback；Codex onboarding 大图则使用终端默认前景色。

---

## Visual Contract

源图为 1254×1254 sRGB PNG。九格中心采样为：

```text
#131E33  #0065FD  #121E31
#01ABFD  #00DDFB  #00DEDF
#01E6CC  #0F1C32  #02E9CD
```

初始 `A` 的亮暗 mask 必须与图标一致：

```text
010
111
101
```

每个终端逻辑方块占 2 列×1 行，格间 1 列，用来补偿字符单元格通常“高大于宽”的比例。完整 brand column 占 13×6：

```text
 ╭────────╮
 │░░ ██ ░░│
 │██ ██ ██│
 │██ ░░ ██│
 ╰────────╯
  ALLTHECODES
```

tracker 行只高亮当前字母索引，因此第一个 `L` 切到第二个 `L` 时仍能看出进度。

### Glyph masks

bit 从左上到右下按 `0brrr_rrr_rrr` 书写：

| Index/letter | Mask | Three rows |
|---|---:|---|
| 0 `A` | `0b010_111_101` | `010 / 111 / 101` |
| 1 `L` | `0b100_100_111` | `100 / 100 / 111` |
| 2 `L` | `0b100_100_111` | `100 / 100 / 111` |
| 3 `T` | `0b111_010_010` | `111 / 010 / 010` |
| 4 `H` | `0b101_111_101` | `101 / 111 / 101` |
| 5 `E` | `0b111_110_111` | `111 / 110 / 111` |
| 6 `C` | `0b111_100_111` | `111 / 100 / 111` |
| 7 `O` | `0b111_101_111` | `111 / 101 / 111` |
| 8 `D` | `0b110_101_110` | `110 / 101 / 110` |
| 9 `E` | `0b111_110_111` | `111 / 110 / 111` |
| 10 `S` | `0b110_010_011` | `110 / 010 / 011` |

### Timing and morph

现有 app tick 为 16ms。`WelcomeLogoState` 每累计 80ms 推进一个 visual step；每个字母占 9 steps：

| Step in segment | Duration | Phase | Cell treatment |
|---:|---:|---|---|
| 0–5 | 480ms | Hold | 当前 mask 为 `██`，暗格为 `░░` |
| 6 | 80ms | FadeOut | outgoing-only 为 `▓▓`，incoming-only 为 `▒▒`，shared 保持 `██` |
| 7 | 80ms | FadeIn | outgoing-only 变 `░░`，incoming-only 为 `▓▓`，shared 保持 `██` |
| 8 | 80ms | Settle | 下一个 mask 全亮，tracker 移到下一个索引 |

每个字母 720ms，完整 11 字母循环 7.92s。`L→L` mask 相同，step 6 将所有亮格降为 `▓▓`，step 7 降为 `▒▒`，step 8 恢复 `██` 并移动 tracker；`S→A` 使用同一 morph 规则闭环。

### Responsive layout

| Available welcome area | Required behavior |
|---|---|
| width ≥48 and height ≥8 | 64 列封顶、8 行面板；左侧 13 列 brand column，2 列 gap，右侧 Version/Model/Session/CWD/Tips |
| width 20–47 and height ≥8 | 保留现有详情面板，不渲染九宫格，不推进动画 redraw |
| width <20 or height <8 | 单行 `allthecodes v<version>` fallback，不越界 |
| initial prompt present | `add_message(User)` 在首次 draw 前关闭 welcome，不出现 logo 闪帧 |
| workspace trust pending | trust gate 优先；logo state 保持 step 0，接受后从 `A` 开始 |

---

## File Map

| File | Responsibility |
|---|---|
| Create `crates/allthecodes/src/ui/components/brand_logo.rs` | glyph 数据、morph 状态机、80ms accumulator、主题 palette、九格和 tracker 渲染、组件测试 |
| Modify `crates/allthecodes/src/ui/mod.rs` | 以 `crate::ui::brand_logo` 暴露新组件 |
| Modify `crates/allthecodes/src/ui/components/welcome.rs` | `WelcomeInfo`、响应式左右布局、详情渲染、welcome snapshots |
| Modify `crates/allthecodes/src/ui/app.rs` | 持有 `WelcomeLogoState` 和可见性，正确暂停/推进/dirty |
| Modify `crates/allthecodes/src/ui/app/render.rs` | 将固定 frame、theme colors 和 welcome info 传给 renderer；记录本帧 logo 是否可见 |
| Modify `crates/allthecodes/src/ui/app/tests.rs` | app 级布局、tick、trust gate、dismissal 测试 |
| Modify `crates/allthecodes/tests/pty_tui_e2e/welcome.rs` | 宽屏真实 logo 与 47 列降级 smoke tests |
| Create `crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__brand_logo__tests__allthecodes_logo_keyframes.snap` | 固定关键帧视觉基线 |
| Create `crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__welcome__tests__welcome_responsive_layouts.snap` | wide/medium/tiny 欢迎页视觉基线 |

---

### Task 1: Build the Deterministic Glyph and Morph Model

**Files:**
- Create: `crates/allthecodes/src/ui/components/brand_logo.rs`
- Modify: `crates/allthecodes/src/ui/mod.rs`
- Test: inline tests in `crates/allthecodes/src/ui/components/brand_logo.rs`

**Interfaces:**
- Consumes: deterministic visual step index (`u64`).
- Produces: `frame_at(step) -> LogoFrame`, immutable `LogoFrame`, `MorphPhase`, and `CellLevel`.

- [ ] **Step 1: Register the module and write failing model tests**

Add beside the other component path modules in `crates/allthecodes/src/ui/mod.rs`:

```rust
#[path = "components/brand_logo.rs"]
pub(crate) mod brand_logo;
```

Create `brand_logo.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_sequence_preserves_duplicate_positions() {
        assert_eq!(WORD.iter().map(|glyph| glyph.letter).collect::<String>(), "ALLTHECODES");
        assert_eq!(WORD[1].mask, WORD[2].mask);
        assert_ne!(frame_at(8).active_index(), frame_at(17).active_index());
    }

    #[test]
    fn source_icon_a_mask_is_exact() {
        assert_eq!(WORD[0].mask, 0b010_111_101);
        assert_eq!(frame_at(0).current_letter(), 'A');
        assert_eq!(frame_at(0).cell_level(0, 1), CellLevel::On);
        assert_eq!(frame_at(0).cell_level(0, 0), CellLevel::Off);
        assert_eq!(frame_at(0).cell_level(2, 1), CellLevel::Off);
    }

    #[test]
    fn a_to_l_uses_two_density_transition_frames() {
        assert_eq!(frame_at(6).phase(), MorphPhase::FadeOut);
        assert_eq!(frame_at(6).cell_level(0, 1), CellLevel::Medium);
        assert_eq!(frame_at(6).cell_level(0, 0), CellLevel::Low);
        assert_eq!(frame_at(7).cell_level(0, 1), CellLevel::Off);
        assert_eq!(frame_at(7).cell_level(0, 0), CellLevel::Medium);
        assert_eq!(frame_at(8).current_letter(), 'L');
        assert_eq!(frame_at(8).cell_level(0, 0), CellLevel::On);
    }

    #[test]
    fn repeated_l_pulses_and_advances_tracker() {
        assert_eq!(frame_at(15).current_letter(), 'L');
        assert_eq!(frame_at(15).cell_level(0, 0), CellLevel::Medium);
        assert_eq!(frame_at(16).cell_level(0, 0), CellLevel::Low);
        assert_eq!(frame_at(17).cell_level(0, 0), CellLevel::On);
        assert_eq!(frame_at(15).active_index(), 1);
        assert_eq!(frame_at(17).active_index(), 2);
    }

}
```

- [ ] **Step 2: Run the focused test and confirm the compile failure**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
```

Expected: FAIL because `WORD`, `frame_at`, `CellLevel`, and `MorphPhase` are not defined.

- [ ] **Step 3: Implement the complete glyph table and pure state machine**

Add above the test module:

```rust
const STEPS_PER_LETTER: u64 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Glyph {
    letter: char,
    mask: u16,
}

const WORD: [Glyph; 11] = [
    Glyph { letter: 'A', mask: 0b010_111_101 },
    Glyph { letter: 'L', mask: 0b100_100_111 },
    Glyph { letter: 'L', mask: 0b100_100_111 },
    Glyph { letter: 'T', mask: 0b111_010_010 },
    Glyph { letter: 'H', mask: 0b101_111_101 },
    Glyph { letter: 'E', mask: 0b111_110_111 },
    Glyph { letter: 'C', mask: 0b111_100_111 },
    Glyph { letter: 'O', mask: 0b111_101_111 },
    Glyph { letter: 'D', mask: 0b110_101_110 },
    Glyph { letter: 'E', mask: 0b111_110_111 },
    Glyph { letter: 'S', mask: 0b110_010_011 },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MorphPhase {
    Hold,
    FadeOut,
    FadeIn,
    Settle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellLevel {
    Off,
    Low,
    Medium,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogoFrame {
    from_index: usize,
    active_index: usize,
    phase: MorphPhase,
}

impl LogoFrame {
    pub(crate) fn active_index(self) -> usize { self.active_index }
    #[cfg(test)]
    pub(crate) fn current_letter(self) -> char { WORD[self.active_index].letter }
    #[cfg(test)]
    pub(crate) fn phase(self) -> MorphPhase { self.phase }

    pub(crate) fn cell_level(self, row: usize, column: usize) -> CellLevel {
        let from = WORD[self.from_index];
        let to = WORD[(self.from_index + 1) % WORD.len()];
        let from_on = mask_has(from.mask, row, column);
        let to_on = mask_has(to.mask, row, column);

        match self.phase {
            MorphPhase::Hold => level(from_on),
            MorphPhase::Settle => level(to_on),
            MorphPhase::FadeOut if from.mask == to.mask => {
                if from_on { CellLevel::Medium } else { CellLevel::Off }
            }
            MorphPhase::FadeIn if from.mask == to.mask => {
                if from_on { CellLevel::Low } else { CellLevel::Off }
            }
            MorphPhase::FadeOut => match (from_on, to_on) {
                (true, true) => CellLevel::On,
                (true, false) => CellLevel::Medium,
                (false, true) => CellLevel::Low,
                (false, false) => CellLevel::Off,
            },
            MorphPhase::FadeIn => match (from_on, to_on) {
                (true, true) => CellLevel::On,
                (true, false) => CellLevel::Off,
                (false, true) => CellLevel::Medium,
                (false, false) => CellLevel::Off,
            },
        }
    }
}

fn level(on: bool) -> CellLevel {
    if on { CellLevel::On } else { CellLevel::Off }
}

fn mask_has(mask: u16, row: usize, column: usize) -> bool {
    debug_assert!(row < 3 && column < 3);
    let bit = 8 - (row * 3 + column);
    mask & (1 << bit) != 0
}

pub(crate) fn frame_at(step: u64) -> LogoFrame {
    let from_index = ((step / STEPS_PER_LETTER) % WORD.len() as u64) as usize;
    let step_in_segment = step % STEPS_PER_LETTER;
    let phase = match step_in_segment {
        0..=5 => MorphPhase::Hold,
        6 => MorphPhase::FadeOut,
        7 => MorphPhase::FadeIn,
        8 => MorphPhase::Settle,
        _ => unreachable!("step modulo nine is always in range"),
    };
    let active_index = if phase == MorphPhase::Settle {
        (from_index + 1) % WORD.len()
    } else {
        from_index
    };
    LogoFrame { from_index, active_index, phase }
}

```

- [ ] **Step 4: Run the focused tests and verify the model passes**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
```

Expected: focused test PASS. Do not run or claim the production warning gate at this intermediate checkpoint; do not add `allow(dead_code)`. Tasks 2–4 complete the production wiring before the required clippy/release verification.

- [ ] **Step 5: Keep the model as the tested checkpoint for Task 2**

Do not commit this intermediate state: the production module is not consumed until its renderer is added. Continue directly to Task 2.

---

### Task 2: Render the Brand Column and Theme Fallbacks

**Files:**
- Modify: `crates/allthecodes/src/ui/components/brand_logo.rs`
- Create: `crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__brand_logo__tests__allthecodes_logo_keyframes.snap`
- Test: inline tests in `crates/allthecodes/src/ui/components/brand_logo.rs`

**Interfaces:**
- Consumes: `Rect`, `Buffer`, `LogoFrame`, `ThemeColors`.
- Produces: `render_brand_logo(area, buf, frame, colors)` and `BRAND_COLUMN_WIDTH/HEIGHT` layout constants.

- [ ] **Step 1: Add failing renderer, palette, and clipping tests**

Add tests that render steps `0, 6, 7, 8, 15, 16, 17, 96, 97, 98` into a 13×6 Buffer, concatenate the labeled frames, and snapshot them:

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use crate::ui::theme::ThemeColors;

#[test]
fn allthecodes_logo_keyframes() {
    let colors = crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::Dark,
    );
    let rendered = [0, 6, 7, 8, 15, 16, 17, 96, 97, 98]
        .into_iter()
        .map(|step| format!("step={step}\n{}", render_for_test(step, colors.colors())))
        .collect::<Vec<_>>()
        .join("\n\n");
    insta::assert_snapshot!("allthecodes_logo_keyframes", rendered);
}

#[test]
fn truecolor_palette_uses_sampled_vertical_gradient() {
    let colors = crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::Dark,
    );
    let palette = BrandPalette::from_theme(colors.colors());
    assert_eq!(palette.active, [
        Color::Rgb(0, 101, 253),
        Color::Rgb(0, 221, 251),
        Color::Rgb(1, 230, 204),
    ]);
}

#[test]
fn ansi_and_reset_palettes_do_not_emit_rgb() {
    let ansi = crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::DarkAnsi,
    );
    let ansi_palette = BrandPalette::from_theme(ansi.colors());
    assert_eq!(ansi_palette.active, [Color::Blue, Color::Cyan, Color::Green]);
    assert_eq!(ansi_palette.inactive, Color::DarkGray);
    assert_eq!(ansi_palette.outline, Color::DarkGray);
    assert_eq!(ansi_palette.tracker, Color::Cyan);

    let mut reset = ansi.colors().clone();
    reset.info = Color::Reset;
    let reset_palette = BrandPalette::from_theme(&reset);
    assert_eq!(reset_palette.active, [Color::Reset; 3]);
    assert_eq!(reset_palette.inactive, Color::Reset);
    assert_eq!(reset_palette.outline, Color::Reset);
    assert_eq!(reset_palette.tracker, Color::Reset);
}

#[test]
fn every_builtin_theme_keeps_lit_and_unlit_cells_distinct() {
    for name in crate::ui::theme::ThemeName::ALL {
        let provider = crate::ui::theme::ThemeProvider::with_name(*name);
        let palette = BrandPalette::from_theme(provider.colors());
        assert!(
            palette.active.iter().all(|active| *active != palette.inactive),
            "theme {name:?} must keep active cells distinct",
        );
    }
}

#[test]
fn undersized_area_is_clipped_without_panicking() {
    let colors = crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::Dark,
    );
    for area in [Rect::new(0, 0, 5, 3), Rect::new(0, 0, 13, 5)] {
        let mut buf = Buffer::empty(area);
        render_brand_logo(area, &mut buf, frame_at(0), colors.colors());
    }
}

#[test]
fn repeated_l_moves_the_tracker_highlight_to_the_second_index() {
    let colors = crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::Dark,
    );
    let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);

    let mut first_l = Buffer::empty(area);
    render_brand_logo(area, &mut first_l, frame_at(8), colors.colors());
    assert!(first_l[(2, 5)]
        .style()
        .add_modifier
        .contains(Modifier::BOLD | Modifier::UNDERLINED));

    let mut second_l = Buffer::empty(area);
    render_brand_logo(area, &mut second_l, frame_at(17), colors.colors());
    assert!(second_l[(3, 5)]
        .style()
        .add_modifier
        .contains(Modifier::BOLD | Modifier::UNDERLINED));
    assert!(!second_l[(2, 5)]
        .style()
        .add_modifier
        .contains(Modifier::UNDERLINED));
}
```

`render_for_test` must convert exactly the requested Buffer rectangle to rows without ANSI escapes so snapshot content is stable.

- [ ] **Step 2: Run tests and confirm missing renderer failures**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
```

Expected: FAIL because `BrandPalette`, `render_brand_logo`, dimensions, and test renderer are absent.

- [ ] **Step 3: Implement the palette and six-line renderer**

Add imports and implementation:

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::theme::ThemeColors;

pub(crate) const BRAND_COLUMN_WIDTH: u16 = 13;
pub(crate) const BRAND_COLUMN_HEIGHT: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrandPalette {
    active: [Color; 3],
    inactive: Color,
    outline: Color,
    tracker: Color,
}

impl BrandPalette {
    fn from_theme(colors: &ThemeColors) -> Self {
        match colors.info {
            Color::Reset => Self {
                active: [Color::Reset; 3],
                inactive: Color::Reset,
                outline: Color::Reset,
                tracker: Color::Reset,
            },
            Color::Rgb(_, _, _) => Self {
                active: [
                    Color::Rgb(0, 101, 253),
                    Color::Rgb(0, 221, 251),
                    Color::Rgb(1, 230, 204),
                ],
                inactive: colors.inactive,
                outline: colors.border,
                tracker: colors.info,
            },
            _ => Self {
                active: [Color::Blue, Color::Cyan, Color::Green],
                inactive: Color::DarkGray,
                outline: Color::DarkGray,
                tracker: Color::Cyan,
            },
        }
    }
}

impl CellLevel {
    fn symbol(self) -> &'static str {
        match self {
            Self::Off => "░░",
            Self::Low => "▒▒",
            Self::Medium => "▓▓",
            Self::On => "██",
        }
    }
}

pub(crate) fn render_brand_logo(
    area: Rect,
    buf: &mut Buffer,
    frame: LogoFrame,
    colors: &ThemeColors,
) {
    let palette = BrandPalette::from_theme(colors);
    let outline = Style::default().fg(palette.outline);
    let mut lines = vec![Line::styled("╭────────╮", outline)];

    for row in 0..3 {
        let mut spans = vec![Span::styled("│", outline)];
        for column in 0..3 {
            if column > 0 {
                spans.push(Span::raw(" "));
            }
            let level = frame.cell_level(row, column);
            let modifier = match level {
                CellLevel::Off | CellLevel::Low => Modifier::DIM,
                CellLevel::Medium => Modifier::empty(),
                CellLevel::On => Modifier::BOLD,
            };
            let color = if level == CellLevel::Off {
                palette.inactive
            } else {
                palette.active[row]
            };
            spans.push(Span::styled(
                level.symbol(),
                Style::default().fg(color).add_modifier(modifier),
            ));
        }
        spans.push(Span::styled("│", outline));
        lines.push(Line::from(spans));
    }
    lines.push(Line::styled("╰────────╯", outline));

    let tracker = WORD.iter().enumerate().map(|(index, glyph)| {
        let style = if index == frame.active_index() {
            Style::default()
                .fg(palette.tracker)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(colors.dim).add_modifier(Modifier::DIM)
        };
        Span::styled(glyph.letter.to_string(), style)
    }).collect::<Vec<_>>();
    lines.push(Line::from(tracker));

    Paragraph::new(lines)
        .alignment(Alignment::Center)
        .render(area, buf);
}
```

Ratatui 自带区域裁剪，因此 5×3 和 13×5 输入不得 panic，也不得向 Buffer area 之外写入。

In the test module, add the exact deterministic snapshot helper used by Step 1:

```rust
fn render_for_test(step: u64, colors: &ThemeColors) -> String {
    let area = Rect::new(0, 0, BRAND_COLUMN_WIDTH, BRAND_COLUMN_HEIGHT);
    let mut buf = Buffer::empty(area);
    render_brand_logo(area, &mut buf, frame_at(step), colors);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .fold(String::new(), |mut row, symbol| {
                    row.push_str(symbol);
                    row
                })
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 4: Generate only the focused snapshot and inspect it**

```bash
INSTA_UPDATE=always cargo test -p allthecodes --bin allthecodes \
  ui::brand_logo::tests::allthecodes_logo_keyframes -- --exact --nocapture
sed -n '1,260p' \
  crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__brand_logo__tests__allthecodes_logo_keyframes.snap
git status --short -- crates/allthecodes/src/ui/components/snapshots/
```

Expected: snapshot 依次显示 A、A→L 两帧、L settle、L→L pulse、S→A 闭环；没有其他 snapshot 改动。

- [ ] **Step 5: Run all brand logo tests**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
```

Expected: PASS with no warnings.

- [ ] **Step 6: Keep the renderer as the tested checkpoint for Task 3**

Do not commit yet: `render_brand_logo` is consumed by production in Task 3. Continue so the eventual feature commit passes `-D warnings`.

---

### Task 3: Add the Responsive Logo to the Welcome Panel

**Files:**
- Modify: `crates/allthecodes/src/ui/components/welcome.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/tests/pty_tui_e2e/welcome.rs`
- Create: `crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__welcome__tests__welcome_responsive_layouts.snap`
- Test: inline tests in `crates/allthecodes/src/ui/components/welcome.rs`
- Test: `crates/allthecodes/tests/pty_tui_e2e/welcome.rs`

**Interfaces:**
- Consumes: `WelcomeInfo<'_>`, fixed `LogoFrame`, `ThemeColors`.
- Produces: `render_welcome(...) -> bool`; `true` means the animated brand column was actually rendered.

- [ ] **Step 1: Replace historical “no logo” assertions with failing responsive tests**

Delete `test_render_welcome_has_no_logo_at_any_width` and the `!content.contains('█')` assertion from `test_render_welcome_normal`. Add:

```rust
#[test]
fn wide_welcome_renders_source_a_and_preserves_details() {
    let area = Rect::new(0, 0, 64, 8);
    let mut buf = Buffer::empty(area);
    let rendered = render_welcome(
        area,
        &mut buf,
        test_info(),
        crate::ui::brand_logo::frame_at(0),
        dark_colors(),
    );
    let content = buf_to_string(&buf, area);
    assert!(rendered);
    assert!(content.contains("░░ ██ ░░"));
    assert!(content.contains("██ ██ ██"));
    assert!(content.contains("██ ░░ ██"));
    assert!(content.contains("ALLTHECODES"));
    assert!(content.contains("Version:"));
    assert!(content.contains("Tips:"));
}

#[test]
fn medium_welcome_keeps_details_without_logo() {
    let area = Rect::new(0, 0, 47, 8);
    let mut buf = Buffer::empty(area);
    let rendered = render_welcome(
        area,
        &mut buf,
        test_info(),
        crate::ui::brand_logo::frame_at(0),
        dark_colors(),
    );
    let content = buf_to_string(&buf, area);
    assert!(!rendered);
    assert!(!content.contains("ALLTHECODES"));
    assert!(content.contains("Version:"));
    assert!(content.contains("Session:"));
}

#[test]
fn welcome_responsive_layouts() {
    insta::assert_snapshot!(
        "welcome_responsive_layouts",
        render_responsive_test_cases([(64, 8), (48, 8), (47, 8), (19, 5), (64, 7)]),
    );
}
```

Add these exact helpers to the same test module, and update every pre-existing `render_welcome` call to pass `test_info()`, `crate::ui::brand_logo::frame_at(0)`, and `dark_colors()`:

```rust
fn test_info() -> WelcomeInfo<'static> {
    WelcomeInfo {
        version: "0.1.0",
        model_name: "claude-sonnet-4",
        session_id: "abcdef1234567890",
        cwd: "/home/user/project",
    }
}

fn dark_colors() -> &'static ThemeColors {
    crate::ui::theme::ThemeProvider::with_name(
        crate::ui::theme::ThemeName::Dark,
    ).colors()
}

fn render_responsive_test_cases<const N: usize>(cases: [(u16, u16); N]) -> String {
    cases
        .into_iter()
        .map(|(width, height)| {
            let area = Rect::new(0, 0, width, height);
            let mut buf = Buffer::empty(area);
            let logo = render_welcome(
                area,
                &mut buf,
                test_info(),
                crate::ui::brand_logo::frame_at(0),
                dark_colors(),
            );
            format!(
                "{width}x{height} logo={logo}\n{}",
                buf_to_string(&buf, area).trim_end(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

Fixed strings and `ThemeName::Dark` make snapshots deterministic.

Add these PTY tests to `crates/allthecodes/tests/pty_tui_e2e/welcome.rs` before the implementation:

```rust
#[test]
fn wide_terminal_shows_nine_grid_logo() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let has_tracker = session.wait_for_screen_text("ALLTHECODES", RENDER_WAIT);
    let screen = session.current_screen();
    let has_grid = screen.contains("╭────────╮") && screen.contains("██");
    let output = session.finish_after_quit("welcome_logo_wide");

    assert!(has_tracker, "wide welcome should show word tracker:\n{screen}");
    assert!(has_grid, "wide welcome should show the 3x3 grid:\n{screen}");
    assert!(!output.contains("panicked"), "logo startup should not panic");
}

#[test]
fn forty_seven_columns_hides_grid_but_keeps_welcome() {
    let session = PtySession::spawn(&default_args(), 47, 24, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let has_wordmark = session.wait_for_screen_text("allthecodes", RENDER_WAIT);
    let screen = session.current_screen();
    let output = session.finish_after_quit("welcome_logo_47_cols");

    assert!(has_wordmark, "narrow welcome should keep its wordmark:\n{screen}");
    assert!(!screen.contains("ALLTHECODES"), "narrow welcome must hide tracker:\n{screen}");
    assert!(!screen.contains("╭────────╮"), "narrow welcome must hide grid:\n{screen}");
    assert!(!output.contains("panicked"), "narrow startup should not panic");
}
```

- [ ] **Step 2: Run welcome tests and verify signature/layout failures**

```bash
cargo test -p allthecodes --test pty_tui_e2e \
  welcome::wide_terminal_shows_nine_grid_logo -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
```

Expected: the PTY test FAILS because the old welcome has no `ALLTHECODES` tracker/grid; the unit test command then FAILS to compile because `WelcomeInfo`, fixed frame/theme parameters, bool return, and responsive helpers are not implemented. The 47-column case is a preservation test and is expected to pass before and after the change.

- [ ] **Step 3: Refactor welcome input and implement the side-by-side layout**

Add:

```rust
use ratatui::layout::{Constraint, Layout};
use crate::ui::brand_logo::{
    render_brand_logo, LogoFrame, BRAND_COLUMN_HEIGHT, BRAND_COLUMN_WIDTH,
};
use crate::ui::theme::ThemeColors;

const LOGO_LAYOUT_MIN_WIDTH: u16 = 48;
const BRAND_GAP: u16 = 2;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WelcomeInfo<'a> {
    pub(crate) version: &'a str,
    pub(crate) model_name: &'a str,
    pub(crate) session_id: &'a str,
    pub(crate) cwd: &'a str,
}
```

Change the public component entry point to:

```rust
pub(crate) fn render_welcome(
    area: Rect,
    buf: &mut Buffer,
    info: WelcomeInfo<'_>,
    logo_frame: LogoFrame,
    colors: &ThemeColors,
) -> bool {
    if area.width < 20 || area.height < PANEL_HEIGHT {
        let line = Line::from(vec![
            Span::styled(
                "allthecodes ",
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("v{}", info.version),
                Style::default().fg(colors.mutedText),
            ),
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
        return false;
    }

    let panel = left_aligned_panel(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.accentDim))
        .title(Line::from(vec![Span::styled(
            " allthecodes ",
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        )]))
        .title_alignment(Alignment::Left);
    let inner = block.inner(panel);
    block.render(panel, buf);

    let show_logo = area.width >= LOGO_LAYOUT_MIN_WIDTH
        && area.height >= PANEL_HEIGHT
        && inner.height >= BRAND_COLUMN_HEIGHT;
    if show_logo {
        let columns = Layout::horizontal([
            Constraint::Length(BRAND_COLUMN_WIDTH),
            Constraint::Length(BRAND_GAP),
            Constraint::Min(0),
        ])
        .split(inner);
        render_brand_logo(columns[0], buf, logo_frame, colors);
        render_info_lines(columns[2], buf, info, colors);
    } else {
        render_info_lines(inner, buf, info, colors);
    }

    show_logo
}
```

Add the complete metadata helper below it:

```rust
fn render_info_lines(
    area: Rect,
    buf: &mut Buffer,
    info: WelcomeInfo<'_>,
    colors: &ThemeColors,
) {
    let raw_model = info
        .model_name
        .strip_prefix("claude-")
        .unwrap_or(info.model_name);
    let max_value_width = area.width.saturating_sub(9) as usize;
    let display_version = truncate_str(&format!("v{}", info.version), max_value_width);
    let display_model = truncate_str(raw_model, max_value_width);
    let short_session = info
        .session_id
        .chars()
        .take(max_value_width.min(8))
        .collect::<String>();
    let display_cwd = truncate_start(info.cwd, max_value_width);
    let tip = truncate_str("Enter to send, /help for commands", max_value_width);
    let label = Style::default().fg(colors.mutedText);
    let value = Style::default().fg(colors.surfaceText);

    let lines = vec![
        Line::from(vec![
            Span::styled("Version: ", label),
            Span::styled(
                display_version,
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model:   ", label),
            Span::styled(display_model, value),
        ]),
        Line::from(vec![
            Span::styled("Session: ", label),
            Span::styled(short_session, value),
        ]),
        Line::from(vec![
            Span::styled("CWD:     ", label),
            Span::styled(display_cwd, label),
        ]),
        Line::from(vec![
            Span::styled("Tips:    ", label),
            Span::styled(tip, value),
        ]),
    ];

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buf);
}
```

Remove the file-local `ACCENT`, `ACCENT_DIM`, `MUTED`, and `LIGHT` constants. The helper calculates `max_value_width` from its own `area.width`, so the logo column cannot make metadata overflow.

Update the existing call in `crates/allthecodes/src/ui/app/render.rs` at this checkpoint so the new signature compiles before animation state is wired:

```rust
let _logo_visible = welcome::render_welcome(
    message_area,
    frame.buffer_mut(),
    welcome::WelcomeInfo {
        version: env!("CARGO_PKG_VERSION"),
        model_name: &self.session_ui.model_name,
        session_id: &self.session_ui.session_id,
        cwd: &self.session_ui.cwd,
    },
    crate::ui::brand_logo::frame_at(0),
    self.design_theme_provider.colors(),
);
```

Task 4 replaces this fixed `A` frame with `self.welcome_logo.frame()` and persists the returned visibility flag.

- [ ] **Step 4: Generate and inspect the focused welcome snapshot**

```bash
INSTA_UPDATE=always cargo test -p allthecodes --bin allthecodes \
  ui::welcome::tests::welcome_responsive_layouts -- --exact --nocapture
sed -n '1,260p' \
  crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__welcome__tests__welcome_responsive_layouts.snap
git status --short -- crates/allthecodes/src/ui/components/snapshots/
```

Expected: 64 and 48 columns show the 3×3 A plus all five details; 47 columns shows details only; 19×5 and 64×7 use the one-line fallback without clipped borders.

- [ ] **Step 5: Run all welcome tests**

```bash
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
cargo test -p allthecodes --test pty_tui_e2e \
  welcome::wide_terminal_shows_nine_grid_logo -- --exact --nocapture
cargo test -p allthecodes --test pty_tui_e2e \
  welcome::forty_seven_columns_hides_grid_but_keeps_welcome -- --exact --nocapture
```

Expected: PASS; `welcome_height_for(20)` and `welcome_height_for(80)` remain 8.

- [ ] **Step 6: Keep the responsive renderer as the tested checkpoint for Task 4**

Do not commit yet: the deterministic `WelcomeLogoState` is connected to production in Task 4. Continue directly so the single cohesive feature commit contains no temporarily unused animation API.

---

### Task 4: Wire Visibility-Aware Animation into App

**Files:**
- Modify: `crates/allthecodes/src/ui/components/brand_logo.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/app/tests.rs`

**Interfaces:**
- Consumes: existing 16ms `AppEvent::Tick`, `show_welcome`, `workspace_trust_pending`, current render area.
- Produces: `WelcomeLogoState::{tick, frame}`, `welcome_logo: WelcomeLogoState`, `welcome_logo_visible: bool`, redraw only when an on-screen frame changes.

- [ ] **Step 1: Add failing App lifecycle tests**

First add this accumulator test to `crates/allthecodes/src/ui/components/brand_logo.rs`:

```rust
#[test]
fn state_advances_only_after_eighty_accumulated_milliseconds() {
    let mut state = WelcomeLogoState::default();
    for _ in 0..4 {
        assert!(!state.tick(16));
    }
    assert!(state.tick(16));
    assert_eq!(state.step(), 1);
    assert!(state.tick(160));
    assert_eq!(state.step(), 3);
}
```

Then add beside `tick_marks_dirty_for_streaming_thinking_animation` in `crates/allthecodes/src/ui/app/tests.rs`:

```rust
#[test]
fn visible_welcome_logo_marks_dirty_every_eighty_ms() {
    let mut app = App::new();
    app.show_welcome = true;
    app.workspace_trust_pending = false;
    app.welcome_logo_visible = true;
    app.dirty = false;

    for _ in 0..4 {
        app.tick();
        assert!(!app.dirty);
    }
    app.tick();
    assert!(app.dirty);
    assert_eq!(app.welcome_logo.step(), 1);
}

#[test]
fn hidden_or_trust_gated_logo_does_not_advance() {
    for (show_welcome, logo_visible, trust_pending) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let mut app = App::new();
        app.show_welcome = show_welcome;
        app.welcome_logo_visible = logo_visible;
        app.workspace_trust_pending = trust_pending;
        app.dirty = false;
        for _ in 0..5 { app.tick(); }
        assert_eq!(app.welcome_logo.step(), 0);
        assert!(!app.dirty);
    }
}

#[test]
fn narrow_render_keeps_welcome_animation_paused() {
    let mut app = App::new();
    let mut terminal = Terminal::new(TestBackend::new(47, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    assert!(!app.welcome_logo_visible);

    app.dirty = false;
    for _ in 0..5 { app.tick(); }
    assert_eq!(app.welcome_logo.step(), 0);
    assert!(!app.dirty);
}

#[test]
fn tiny_render_clears_stale_logo_visibility_before_returning() {
    let mut app = App::new();
    app.welcome_logo_visible = true;
    let mut terminal = Terminal::new(TestBackend::new(9, 3)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    assert!(!app.welcome_logo_visible);
}

#[test]
fn first_conversation_message_stops_welcome_animation() {
    let mut app = App::new();
    app.welcome_logo_visible = true;
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text("hello".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));
    assert!(!app.show_welcome);
    assert!(!app.welcome_logo_visible);
}
```

Extend `render_places_prompt_after_compact_welcome` with this assertion while retaining the existing row 8 blank and row 10 prompt assertions:

```rust
assert!(
    content[..8].iter().any(|line| line.contains("ALLTHECODES")),
    "wide welcome should render the animated word tracker",
);
assert!(app.welcome_logo_visible);
```

- [ ] **Step 2: Run the App tests and confirm missing state/signature failures**

```bash
cargo test -p allthecodes --bin allthecodes \
  ui::brand_logo::tests::state_advances_only_after_eighty_accumulated_milliseconds \
  -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes \
  ui::app::tests::visible_welcome_logo_marks_dirty_every_eighty_ms \
  -- --exact --nocapture
```

Expected: the brand test FAILS because `WelcomeLogoState` does not exist; the App test FAILS because `welcome_logo` and `welcome_logo_visible` do not exist.

- [ ] **Step 3: Add the 80ms accumulator, App state, and tick gating**

Add the accumulator to `crates/allthecodes/src/ui/components/brand_logo.rs`:

```rust
pub(crate) const FRAME_INTERVAL_MS: u64 = 80;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WelcomeLogoState {
    step: u64,
    carry_ms: u64,
}

impl WelcomeLogoState {
    pub(crate) fn tick(&mut self, elapsed_ms: u64) -> bool {
        self.carry_ms = self.carry_ms.saturating_add(elapsed_ms);
        let advances = self.carry_ms / FRAME_INTERVAL_MS;
        self.carry_ms %= FRAME_INTERVAL_MS;
        self.step = self.step.wrapping_add(advances);
        advances > 0
    }

    pub(crate) fn frame(&self) -> LogoFrame { frame_at(self.step) }

    #[cfg(test)]
    pub(crate) fn step(&self) -> u64 { self.step }
}
```

In `App`, initialize these fields in `App::new()`:

```rust
welcome_logo: WelcomeLogoState,
welcome_logo_visible: bool,
```

```rust
welcome_logo: WelcomeLogoState::default(),
welcome_logo_visible: false,
```

Import the state in `crates/allthecodes/src/ui/app.rs`:

```rust
use crate::ui::brand_logo::WelcomeLogoState;
```

Then add to `App::tick()` after incrementing `tick_counter`:

```rust
if self.show_welcome
    && self.welcome_logo_visible
    && !self.workspace_trust_pending
    && self.welcome_logo.tick(16)
{
    self.dirty = true;
}
```

When `add_message` dismisses the welcome page, set both:

```rust
self.show_welcome = false;
self.welcome_logo_visible = false;
```

Do not reset state on dismissal because welcome is one-shot. Trust gating does not call `tick`, so the first visible frame remains `A`.

- [ ] **Step 4: Pass deterministic frame and theme data from App::render**

Immediately after `let size = frame.area();` and before the existing `<10×4` early return, trust gate, and transcript/focus branches, set:

```rust
self.welcome_logo_visible = false;
```

Replace Task 3's temporary `_logo_visible` call with:

```rust
let logo_visible = welcome::render_welcome(
    message_area,
    frame.buffer_mut(),
    welcome::WelcomeInfo {
        version: env!("CARGO_PKG_VERSION"),
        model_name: &self.session_ui.model_name,
        session_id: &self.session_ui.session_id,
        cwd: &self.session_ui.cwd,
    },
    self.welcome_logo.frame(),
    self.design_theme_provider.colors(),
);
self.welcome_logo_visible = logo_visible;
```

This ordering guarantees trust, transcript/focus, tiny terminal, and medium terminal paths leave `welcome_logo_visible == false`.

- [ ] **Step 5: Run component and App regression tests**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::render_places_prompt_after_compact_welcome -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::visible_welcome_logo_marks_dirty_every_eighty_ms -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::hidden_or_trust_gated_logo_does_not_advance -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::narrow_render_keeps_welcome_animation_paused -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::tiny_render_clears_stale_logo_visibility_before_returning -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::first_conversation_message_stops_welcome_animation -- --exact --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::tick_marks_dirty_for_streaming_thinking_animation -- --exact --nocapture
```

Expected: all PASS; spinner/thinking tick behavior remains unchanged.

- [ ] **Step 6: Keep the complete feature uncommitted for final verification**

Do not commit before Task 5. The repository instructions require the full release build and warning checks before commit; Task 5 runs those gates and then creates the explicit-path feature commit.

---

### Task 5: Verify Terminal Behavior and Full Build

**Files:**
- Verify: `crates/allthecodes/tests/pty_tui_e2e/welcome.rs` and the unchanged PTY harness around it
- Verify: all implementation paths from Tasks 1–4

**Interfaces:**
- Consumes: completed component, welcome, and App integration.
- Produces: evidence that focused tests, terminal smoke, formatting, clippy, workspace tests, and release build succeed without warnings.

- [ ] **Step 1: Check scope before verification**

```bash
git status --short
git diff --check -- \
  crates/allthecodes/src/ui/components/brand_logo.rs \
  crates/allthecodes/src/ui/components/welcome.rs \
  crates/allthecodes/src/ui/mod.rs \
  crates/allthecodes/src/ui/app.rs \
  crates/allthecodes/src/ui/app/render.rs \
  crates/allthecodes/src/ui/app/tests.rs \
  crates/allthecodes/tests/pty_tui_e2e/welcome.rs \
  crates/allthecodes/src/ui/components/snapshots/
if rg -n '[[:blank:]]+$' \
  crates/allthecodes/src/ui/components/brand_logo.rs \
  crates/allthecodes/src/ui/components/welcome.rs \
  crates/allthecodes/src/ui/components/snapshots/; then
  exit 1
fi
```

Expected: no whitespace errors; unrelated dirty paths remain untouched.

- [ ] **Step 2: Run focused deterministic tests once more**

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests -- --nocapture
```

Expected: PASS. Snapshot tests must not create `.snap.new` files.

- [ ] **Step 3: Run the PTY welcome smoke suite**

```bash
cargo test -p allthecodes --test pty_tui_e2e welcome -- --nocapture
```

Expected: startup prompt, ready/model checks, 47-column fallback, wide logo, 80×24 and 200×50 no-crash cases pass. Deterministic morph timing remains in Buffer/App tests because the PTY harness has no fixed clock.

- [ ] **Step 4: Verify formatting and warnings**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both exit 0; do not suppress unused code/import warnings.

- [ ] **Step 5: Run workspace tests with the documented PTY stall checks**

```bash
cargo test --workspace
```

Expected: exit 0. If execution reaches `pty_tui_e2e commands_mcp_plugin`, monitor the active log and cargo/rustc/bridge descendants per `AGENTS.md`; do not report the workspace suite as passing unless the command returns 0.

- [ ] **Step 6: Run the mandatory release build**

```bash
cargo build --workspace --release
```

Expected: exit 0 with no new warning.

- [ ] **Step 7: Inspect the final animation manually**

Launch the normal TUI from a terminal at least 64×24:

```bash
cargo run -p allthecodes -- -C "$PWD" --permission-mode bypass
```

Then verify:

1. trust gate, when present, shows before the logo;
2. after trust, the first mask is the source icon `A`;
3. tracker advances through all 11 positions in about 7.92 seconds;
4. the two consecutive `L` positions are separated by a density pulse;
5. resizing below 48 columns removes the logo but keeps all metadata and stops periodic redraw;
6. resizing back to at least 48 columns restores the current frame without stale cells;
7. sending the first prompt removes the entire welcome panel and stops logo redraw.

- [ ] **Step 8: Final explicit-path status check**

```bash
git status --short
git diff --stat -- \
  crates/allthecodes/src/ui/components/brand_logo.rs \
  crates/allthecodes/src/ui/components/welcome.rs \
  crates/allthecodes/src/ui/mod.rs \
  crates/allthecodes/src/ui/app.rs \
  crates/allthecodes/src/ui/app/render.rs \
  crates/allthecodes/src/ui/app/tests.rs \
  crates/allthecodes/tests/pty_tui_e2e/welcome.rs \
  crates/allthecodes/src/ui/components/snapshots/
```

Expected: only the intended TUI implementation paths appear in the feature diff; the pre-existing user changes remain uncommitted and unstaged by this work.

- [ ] **Step 9: Commit only the verified feature paths**

```bash
git config user.name "Crsei"
git config user.email "Crsei@protonmail.com"
git add -A -- \
  crates/allthecodes/src/ui/components/brand_logo.rs \
  crates/allthecodes/src/ui/components/welcome.rs \
  crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__brand_logo__tests__allthecodes_logo_keyframes.snap \
  crates/allthecodes/src/ui/components/snapshots/allthecodes__ui__welcome__tests__welcome_responsive_layouts.snap \
  crates/allthecodes/src/ui/mod.rs \
  crates/allthecodes/src/ui/app.rs \
  crates/allthecodes/src/ui/app/render.rs \
  crates/allthecodes/src/ui/app/tests.rs \
  crates/allthecodes/tests/pty_tui_e2e/welcome.rs
git commit -m "Add the animated TUI welcome logo"
```

Expected: commit succeeds after all Task 5 verification commands have returned 0. Do not push unless the user separately requests it.

---

## Acceptance Matrix

| Concern | Automated evidence | Required result |
|---|---|---|
| Exact word order | `word_sequence_preserves_duplicate_positions` | `ALLTHECODES`, length 11, duplicate L indices preserved |
| Source icon fidelity | `source_icon_a_mask_is_exact` | `010/111/101` at step 0 |
| Morph behavior | A→L, L→L, S→A keyframe tests/snapshot | density steps match the timing table |
| Wide layout | 64×8 and 48×8 welcome snapshot | logo and five metadata rows coexist within 8 lines |
| Narrow fallback | 47×8, 19×5, 64×7 snapshot | no logo, no clipping, useful text remains |
| Theme behavior | truecolor/ANSI/reset palette tests | exact sampled RGB, no RGB in ANSI/reset modes |
| Redraw budget | App tick tests | dirty at most once per 80ms and only when logo is on screen |
| Lifecycle | trust/initial prompt/first message tests | no hidden animation, no startup flash, first visible glyph A |
| Prompt placement | existing App render test | spacer remains row 8, prompt remains row 10 at 80×24 |
| Terminal smoke | existing PTY welcome suite | startup/small/wide cases exit without panic |
| Submission quality | fmt, clippy, workspace tests, release build | all required commands exit 0 with no new warning |

## Completion Definition

- 九宫格在宽屏欢迎页中依序显示 `ALLTHECODES`，重复字母和闭环转换可观察、可测试。
- 视觉样式保留源图的 A mask、暗格、圆角小框和蓝→cyan→teal 色流。
- 8 行欢迎面板、详情字段、prompt 布局、首次消息 dismissal 与 theme switching 均无回归。
- 隐藏 logo 不产生周期 redraw；所有测试使用固定 step，不依赖 wall clock 或随机数。
- focused tests、PTY smoke、fmt、clippy、workspace tests 和 workspace release build 均有 exit-0 证据。
