# Allthecodes TUI Integrated Glyph Logo Plan

> **Status:** Monochrome geometry review
>
> **Supersedes:** The visual contract in
> [2026-07-10-allthecodes-tui-logo-implementation-plan.md](2026-07-10-allthecodes-tui-logo-implementation-plan.md).
> The old plan remains the implementation record for the released 0.1.13 logo.

## Goal

在 Rust TUI 欢迎页的九宫格内部依次显示 `ALLTHECODES` 字母。字母不再依赖
九宫格下方的独立 tracker；每个逻辑方块允许只填充局部多边形，并在字母切换时
逐步改变格内覆盖形状。

第一阶段只交付无彩色版本，用终端默认前景/背景验证字形、节奏与兼容性。品牌色、
发光和色彩渐变必须等单色几何确认后再作为独立样式层设计，不能重新耦合字形数据。

## Why the 0.1.13 Contract Is Replaced

实际终端复查确认旧实现严格执行了旧计划，但旧合同有三项问题：

1. `ALLTHECODES` tracker 位于九宫格外，视觉上形成“图形 + 字体”两个对象。
2. 每个字母只有 3×3 二值 mask；`░░/▒▒/▓▓/██` 只能改变整格密度，不能表达
   斜边、拐角、圆弧或格内缺口。
3. 字母间只有两个 80ms 密度过渡帧，变化更像块状态切换，不像形状渐变。

Ratatui 不能在字符单元里直接绘制任意矢量路径。本计划将每格的 2×2 子区分别映射
到两个 Unicode half-block 字符；这是固定宽度终端内可测试、无需字体资源的实现边界。

## Monochrome Visual Contract

### Integrated 3×3 grid

- 外框和三个逻辑行保持紧凑的 10×5 实际图案；brand layout column 仍保留 13 列，
  避免右侧 Version/Model/Session/CWD/Tips 横向跳动。
- 九个逻辑方块各自覆盖 2×2 子区；左右子区列分别用 ` `、`▀`、`▄`、`█`
  表达空、上半、下半和全高填充，避免把同一个 quadrant 图案横向平铺两次。
- 每个块正好占两个终端列，并以一列间隙维持接近正方形的物理比例。
- 稳定字形使用 6×6 单色子像素数据，再按 2×2 分组映射到九宫格。
- 不渲染独立大写 `ALLTHECODES` tracker。面板标题中的小写 `allthecodes` 保留，
  因为它是产品/面板标题，不参与动画字形。

首帧 `A`：

```text
╭────────╮
│ ▄ ▀▀ ▄ │
│█▄ ▄▄ ▄█│
│█      █│
╰────────╯
```

曲线字母可以使用局部块，例如 `O`：

```text
╭────────╮
│ ▄ ██ ▄ │
│██    ██│
│ ▀ ██ ▀ │
╰────────╯
```

### Monochrome style

- logo 外框、块元素和空白都使用 ratatui 默认样式。
- logo 不设置 RGB、ANSI named color、foreground、background 或品牌 palette。
- 欢迎面板和右侧信息继续使用现有主题；“无彩色”只限定本轮 logo 图案。
- `NO_COLOR`、ANSI、truecolor 和未知主题下的 logo 几何必须完全一致。

### Morph timing

现有 `AppEvent::Tick`、80ms accumulator 和“仅在宽屏 welcome 实际可见时推进”的
生命周期保持不变。每个字母改为 12 steps：

| Segment step | Duration | Treatment |
| ---: | ---: | --- |
| 0–5 | 480ms | 保持当前稳定字形 |
| 6–10 | 400ms | 五个格内几何渐变帧；按每对字形的实际差异像素均摊五批，并保持中心到外缘的稳定顺序 |
| 11 | 80ms | target settle，并推进内部字母索引 |

完整 11 字母循环为 10.56s。连续两个 `L` 保留两个索引；因为几何相同，转换段使用
单色子像素收缩—恢复 pulse，不能依赖已删除的 tracker 表示进度。`S→A` 使用同一
中心向外规则闭环。

## Responsive and Lifecycle Contract

- width ≥48、height ≥8：显示 13 列 brand column 与右侧详情。
- width 20–47、height ≥8：隐藏 logo、保留详情、停止 logo redraw。
- width <20 或 height <8：保留单行 fallback，停止 logo redraw。
- initial prompt、首个 user/assistant message、workspace trust gate、transcript/focus view
  均继续复用现有 welcome visibility gating。
- 欢迎面板继续为 8 行；删除 tracker 后留下的底部内边距不挪动 prompt，避免启动布局
  发生纵向跳动。

## File Map

| File | Change |
| --- | --- |
| `crates/allthecodes/src/ui/components/brand_logo.rs` | 6×6 glyph、half-block 子区映射、逐 transition 五帧 morph、重复 L pulse、单色 renderer 与组件测试 |
| `crates/allthecodes/src/ui/components/welcome.rs` | 调用单色 renderer，更新格内字形断言 |
| `crates/allthecodes/src/ui/app/tests.rs` | 保留 prompt/lifecycle 检查，删除 tracker 假设 |
| `crates/allthecodes/tests/pty_tui_e2e/welcome.rs` | 真实宽屏验九宫格、partial block、五行边界列对齐，且外置 tracker 不存在 |
| `crates/allthecodes/src/ui/components/snapshots/` | 11 个稳定字形、A→L/L→L/O→D/S→A 关键帧和 responsive welcome 基线 |

`app.rs` 与 `app/render.rs` 的生产生命周期接线无需修改；这也避免覆盖共享工作区中
这些文件上的其他并行改动。

## Tasks

- [x] 复现 0.1.13 无彩运行态并确认“九宫格 + 外置 tracker”来自旧计划。
- [x] 将旧计划标为历史合同并建立本 follow-up 计划。
- [x] 用 6×6 子像素和两列 half-blocks 定义 11 个稳定字形。
- [x] 删除外置 tracker 和 logo palette，保留默认终端样式。
- [x] 将两帧密度切换替换为五帧几何渐变，并为第二个 `L` 增加 pulse。
- [x] 更新全部组件、welcome、App 与 PTY 测试及 snapshots。
- [x] 运行 fmt、针对性测试和真实 PTY 多帧抓取。
- [ ] 清除当前非 Logo 路径的严格 clippy 阻塞后重跑完整 `-D warnings` 门禁。
- [ ] 由用户确认单色字形与节奏；确认前不实现颜色阶段。

## Verification

```bash
cargo test -p allthecodes --bin allthecodes ui::brand_logo::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::welcome::tests -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::app::tests::render_places_prompt_after_compact_welcome -- --exact --nocapture
cargo test -p allthecodes --test pty_tui_e2e welcome::wide_terminal_shows_integrated_nine_grid_logo -- --exact --nocapture
cargo test -p allthecodes --test pty_tui_e2e welcome::forty_seven_columns_hides_grid_but_keeps_welcome -- --exact --nocapture
cargo fmt --all --check
cargo clippy -p allthecodes --bin allthecodes --tests -- -D warnings
```

2026-07-16 当前证据：brand tests 10/10、welcome tests 9/9、App tests 77/77、上述
两个 PTY cases、`cargo build -p allthecodes`、`cargo fmt --all --check` 与多帧 tmux
抓取均通过；在只放行下述两个非 Logo lint 后，package `--no-deps -D warnings` 也通过。
原始严格 clippy 尚被
非 Logo 路径 `ui/app/domain.rs:223` 的 `obfuscated_if_else`、
`ui/permissions/dialog_overlay.rs:538` 的 `expect_used` 阻塞；完整 dependency lint 还会在
`allthecodes-engine/src/agent/fork_context.rs` 与 `live_parent_context.rs` 报告既有
`expect_used`。本任务不覆盖这些路径。

此外必须用临时 `ALLTHECODES_HOME` 在至少 120×40 的真实终端抓取多个时刻，确认：

- 字母只在九宫格内；
- 不存在独立大写 tracker；
- partial block 宽度稳定，没有 Unicode double-width 错位；
- A→L、L→L、S→A 肉眼可见为多帧渐变；
- logo 图案没有显式颜色 escape/style。

## Completion Boundary

本阶段完成只表示“无彩色 integrated glyph geometry”通过代码与运行态验证。颜色、发光、
阴影、用户 motion preference 和可配置动画仍是后续设计，不得在用户确认本版形状前混入。
