# Allthecodes TUI 九宫格紧凑间距计划

> **Status:** Approved for implementation
>
> **Follow-up to:** [2026-07-16-allthecodes-tui-logo-integrated-glyph-plan.md](2026-07-16-allthecodes-tui-logo-integrated-glyph-plan.md)

## Goal

移除 `brand_logo` 九宫格三个逻辑列之间的渲染空白，并将 logo 外框与布局列宽收紧到
实际的 6 个终端子像素列。字形内部因为像素为空产生的空白仍必须保留；本任务只删除
逻辑列之间的固定分隔符。

## Visual contract

- 每个逻辑格继续占两个终端列；3×3 网格的内容宽度为 `3 × 2 = 6` 列。
- 外框改为 8 列：`╭──────╮` / `╰──────╯`，内部每行是左边框、6 列字形、右边框。
- `BRAND_COLUMN_WIDTH` 改为 `8`，因此不再通过 13 列区域居中产生额外左右留白。
- `BRAND_GAP = 2` 保留，它只分隔完整 logo 与右侧 welcome 信息，不是字形列间距。
- 6×6 字形、half-block 映射、80ms 动画、五帧 morph、无颜色样式和 responsive 门槛
  保持不变。

示例首帧 `A`：

```text
╭──────╮
│ ▄▀▀▄ │
│█▄▄▄▄█│
│█    █│
╰──────╯
```

## Files

| File | Change |
| --- | --- |
| `crates/allthecodes/src/ui/components/brand_logo.rs` | 删除列间 `Span::raw(" ")`，将外框和 `BRAND_COLUMN_WIDTH` 收紧，并增加紧凑宽度断言。 |
| `crates/allthecodes/src/ui/components/welcome.rs` | 更新宽屏字形断言。 |
| `crates/allthecodes/src/ui/app/tests.rs` | 更新 welcome 内嵌字形断言。 |
| `crates/allthecodes/tests/pty_tui_e2e/welcome.rs` | 更新边框文本与右边界列断言。 |
| `crates/allthecodes/src/ui/components/snapshots/` | 更新 logo 与 welcome 快照。 |

## Verification

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::render_places_prompt_after_compact_welcome -- --exact --nocapture
cargo test -p allthecodes --test pty_tui_e2e welcome::wide_terminal_shows_integrated_nine_grid_logo -- --exact --nocapture
cargo fmt --all --check
cargo build --workspace --release --locked
```
