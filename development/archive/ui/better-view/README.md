# Better View UI Snapshots

日期: 2026-05-10

本目录记录 command 管理类 TUI 的目标视觉形态。它不是当前运行时录屏，也不是现有 `insta` 输出；它是后续把纯文本菜单升级为统一面板体系时使用的理想 snapshot 合约。

## 目标

- 将 `Title + tabs + > item - description` 的纯文本菜单升级为统一的设置面板。
- 保留现有命令行为、快捷键、提交命令和 picker 语义。
- 按 [../../COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) 的分类覆盖设置类、选择器类、向导类、外部流程和审批类。
- 让 snapshot 能直接指导后续 ratatui renderer 设计，而不是只描述文案。

## 面板约定

- 左侧为 section nav，负责稳定定位。
- 右侧为当前 section 的 settings/detail panel。
- 底部为固定 shortcut footer，不随选中项重复堆叠。
- 选中行仍用 `>` 表示；当前值用 `current`；配置来源用 `source=<layer>`。
- 危险或高权限动作必须显式带 `risk=<level>` 或 `warning`。
- Picker 仍是 picker，不强行改成普通表格；但要放在统一面板框架内。

## 文件索引

| 文件 | 覆盖范围 |
| --- | --- |
| [settings-panels.md](settings-panels.md) | 设置类命令的统一设置面板目标 snapshots |
| [selector-surfaces.md](selector-surfaces.md) | 选择器类入口的统一列表/详情面板目标 snapshots |
| [wizard-flows.md](wizard-flows.md) | 向导类和多步骤流程的目标 snapshots |
| [external-flows.md](external-flows.md) | 外部页面、编辑器、IDE/App 流程的目标 snapshots |
| [approval-panels.md](approval-panels.md) | 非设置确认/审批类 UI 的目标 snapshots |

## 分类映射

| `COMMAND_UI_REFERENCE.md` 分类 | Better View 文件 |
| --- | --- |
| 一、设置类 | [settings-panels.md](settings-panels.md) |
| 二、选择器类 | [selector-surfaces.md](selector-surfaces.md) |
| 三、向导类 | [wizard-flows.md](wizard-flows.md) |
| 四、外部页面、二维码或 App | [external-flows.md](external-flows.md) |
| 五、非设置界面的确认/审批类 | [approval-panels.md](approval-panels.md) |
