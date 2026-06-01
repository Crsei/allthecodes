# Codex 上游 TUI 结构梳理

根目录：`F:\AIclassmanager\cc\codex\codex-rs\tui`

来源：`gpt-5.5` subagent 只读梳理。未修改文件，未启动构建或测试。`rg` 在当前环境被拒绝执行，改用 PowerShell 递归枚举。

## 入口与配置

```text
tui/
├─ BUILD.bazel                         # Bazel 构建定义
├─ Cargo.toml                          # Rust crate 定义；lib=codex_tui，bin=codex-tui、md-events
├─ prompt_for_init_command.md          # 初始化命令提示模板/文案资源（推断）
├─ styles.md                           # TUI 样式说明/设计文档（推断）
├─ tooltips.txt                        # tooltip 文案资源
├─ src/main.rs                         # codex-tui 二进制入口
├─ src/lib.rs                          # codex_tui 库入口，声明并装配主要模块
└─ src/bin/md-events.rs                # md-events 辅助二进制入口
```

## 主要源码树

```text
src/
├─ additional_dirs.rs                  # 额外目录警告文案
├─ app.rs                              # App 主状态/主循环核心模块
├─ app/
│  ├─ agent_navigation.rs              # 多代理/线程导航状态
│  ├─ app_server_adapter.rs            # App 与 app-server 适配逻辑
│  ├─ app_server_requests.rs           # app-server pending 请求管理
│  ├─ loaded_threads.rs                # 已加载线程集合
│  └─ pending_interactive_replay.rs    # 交互式请求重放状态
├─ app_backtrack.rs                    # App 回退/历史导航逻辑
├─ app_command.rs                      # AppCommand 命令枚举/解析
├─ app_event.rs                        # AppEvent 事件类型
├─ app_event_sender.rs                 # App 事件发送封装
├─ app_server_approval_conversions.rs  # app-server 审批类型转换
├─ app_server_session.rs               # app-server 会话生命周期
├─ ascii_animation.rs                  # ASCII 动画加载/播放
├─ audio_device.rs                     # 实时音频设备枚举与选择
├─ chatwidget.rs                       # 主聊天界面模块入口
├─ cli.rs                              # TUI CLI 参数/启动配置
├─ clipboard_paste.rs                  # 粘贴事件处理
├─ clipboard_text.rs                   # 剪贴板文本访问
├─ collaboration_modes.rs              # 协作/代理模式定义
├─ color.rs                            # 颜色工具
├─ custom_terminal.rs                  # 自定义终端能力封装
├─ cwd_prompt.rs                       # 工作目录选择/信任提示
├─ debug_config.rs                     # 调试配置
├─ diff_render.rs                      # diff / apply_patch 渲染
├─ exec_command.rs                     # 命令执行展示模型
├─ external_editor.rs                  # 外部编辑器集成
├─ file_search.rs                      # 文件搜索支持
├─ frames.rs                           # frames/ 动画资源加载
├─ get_git_diff.rs                     # git diff 获取
├─ history_cell.rs                     # 聊天历史单元格渲染
├─ insert_history.rs                   # 插入历史记录支持
├─ key_hint.rs                         # 键位提示模型
├─ line_truncation.rs                  # 行截断工具
├─ live_wrap.rs                        # 实时文本换行
├─ local_chatgpt_auth.rs               # 本地 ChatGPT 认证辅助
├─ markdown.rs                         # Markdown 基础解析/模型
├─ markdown_render.rs                  # Markdown 到 ratatui 渲染
├─ markdown_render_tests.rs            # Markdown 渲染单元测试
├─ markdown_stream.rs                  # 流式 Markdown 处理
├─ mention_codec.rs                    # mention 编码/解码
├─ model_catalog.rs                    # 模型目录/展示数据
├─ model_migration.rs                  # 模型迁移提示
├─ multi_agents.rs                     # 多代理 UI/转录逻辑
├─ oss_selection.rs                    # OSS provider 选择 UI
├─ pager_overlay.rs                    # 分页覆盖层/转录查看器
├─ resume_picker.rs                    # 会话恢复选择器
├─ selection_list.rs                   # 通用选择列表（推断）
├─ session_log.rs                      # 会话日志写入
├─ shimmer.rs                          # shimmer 动效样式
├─ skills_helpers.rs                   # skills 辅助函数（推断）
├─ slash_command.rs                    # 斜杠命令定义
├─ status_indicator_widget.rs          # 顶部/底部状态指示组件
├─ style.rs                            # 主题样式函数
├─ terminal_palette.rs                 # 终端调色板检测/映射
├─ terminal_title.rs                   # 终端标题设置
├─ test_backend.rs                     # VT100 测试 backend
├─ test_support.rs                     # 测试路径辅助
├─ text_formatting.rs                  # 文本格式化工具（推断）
├─ theme_picker.rs                     # 主题选择器
├─ tooltips.rs                         # tooltip 选择/展示逻辑
├─ tui.rs                              # 终端模式、alt-screen、TUI 运行入口
├─ ui_consts.rs                        # UI 常量
├─ update_action.rs                    # 更新动作检测
├─ update_prompt.rs                    # 更新提示弹窗
├─ updates.rs                          # 版本检查/升级信息
├─ version.rs                          # crate 版本常量
├─ voice.rs                            # 实时语音输入/输出
└─ wrapping.rs                         # 文本换行范围映射
```

## 子模块边界

```text
src/bottom_pane/
├─ AGENTS.md                          # bottom_pane 局部指导
├─ mod.rs                             # bottom pane 模块出口
├─ bottom_pane_view.rs                # BottomPaneView trait/视图接口（推断）
├─ chat_composer.rs                   # 输入框/聊天 composer
├─ chat_composer_history.rs           # composer 历史
├─ command_popup.rs                   # slash 命令弹窗
├─ approval_overlay.rs                # 命令/patch 审批覆盖层
├─ app_link_view.rs                   # app 连接/安装建议视图
├─ custom_prompt_view.rs              # 自定义 prompt 视图
├─ experimental_features_view.rs      # 实验功能开关视图
├─ feedback_view.rs                   # 反馈表单视图
├─ file_search_popup.rs               # 文件搜索弹窗
├─ footer.rs                          # footer/快捷键/状态行
├─ list_selection_view.rs             # 通用列表选择视图
├─ mcp_server_elicitation.rs          # MCP server elicitation 表单
├─ multi_select_picker.rs             # 多选 picker
├─ paste_burst.rs                     # 快速粘贴检测
├─ pending_input_preview.rs           # 待发送输入预览
├─ pending_thread_approvals.rs        # 待审批线程提示
├─ popup_consts.rs                    # 弹窗尺寸/布局常量
├─ prompt_args.rs                     # slash prompt 参数解析
├─ scroll_state.rs                    # 滚动状态
├─ selection_popup_common.rs          # 选择弹窗公共布局
├─ skill_popup.rs                     # skill mention 弹窗
├─ skills_toggle_view.rs              # skills 开关视图
├─ slash_commands.rs                  # bottom pane slash 命令适配
├─ status_line_setup.rs               # 状态行设置视图
├─ textarea.rs                        # 文本输入组件
├─ title_setup.rs                     # 终端标题设置视图
├─ unified_exec_footer.rs             # 统一执行状态 footer
└─ request_user_input/
   ├─ mod.rs                          # request_user_input overlay 状态
   ├─ layout.rs                       # request_user_input 布局计算
   └─ render.rs                       # request_user_input 渲染
```

```text
src/chatwidget/
├─ tests.rs                           # chatwidget 测试入口
├─ interrupts.rs                      # 中断处理
├─ plugins.rs                         # 插件弹窗/状态逻辑
├─ realtime.rs                        # 实时音频/会话 UI
├─ session_header.rs                  # 会话头部渲染
├─ skills.rs                          # skills 相关 UI
├─ status_surfaces.rs                 # 状态展示面
└─ tests/
   ├─ app_server.rs                   # app-server 交互测试
   ├─ approval_requests.rs            # 审批请求测试
   ├─ background_events.rs            # 后台事件测试
   ├─ composer_submission.rs          # composer 提交测试
   ├─ exec_flow.rs                    # 执行流测试
   ├─ guardian.rs                     # guardian 审查测试
   ├─ helpers.rs                      # chatwidget 测试辅助
   ├─ history_replay.rs               # 历史重放测试
   ├─ mcp_startup.rs                  # MCP 启动测试
   ├─ permissions.rs                  # 权限模式测试
   ├─ plan_mode.rs                    # plan mode 测试
   ├─ popups_and_settings.rs          # 弹窗/设置测试
   ├─ review_mode.rs                  # review mode 测试
   ├─ slash_commands.rs               # slash 命令测试
   ├─ status_and_layout.rs            # 状态/布局测试
   └─ status_command_tests.rs         # /status 命令测试
```

```text
src/exec_cell/
├─ mod.rs                             # exec cell 模块出口
├─ model.rs                           # 执行单元数据模型
└─ render.rs                          # 执行单元渲染

src/notifications/
├─ mod.rs                             # 通知模块出口
├─ bel.rs                             # BEL 终端通知
└─ osc9.rs                            # OSC 9 终端通知

src/onboarding/
├─ mod.rs                             # onboarding 模块出口
├─ auth.rs                            # onboarding 登录步骤
├─ auth/headless_chatgpt_login.rs     # headless ChatGPT 登录
├─ onboarding_screen.rs               # onboarding 流程驱动
├─ trust_directory.rs                 # 信任目录选择步骤
└─ welcome.rs                         # 欢迎页/动画步骤

src/public_widgets/
├─ mod.rs                             # 公共 widgets 出口
└─ composer_input.rs                  # 可复用 composer input

src/render/
├─ mod.rs                             # 渲染工具出口、Insets/RectExt
├─ highlight.rs                       # syntect/two-face 语法高亮
├─ line_utils.rs                      # Line 转换/前缀工具
└─ renderable.rs                      # Renderable trait 与组合渲染

src/status/
├─ mod.rs                             # /status 模块出口
├─ account.rs                         # 账号显示数据
├─ card.rs                            # status 卡片/历史 cell
├─ format.rs                          # 字段格式化
├─ helpers.rs                         # status 辅助格式化
├─ rate_limits.rs                     # rate limit/credits 展示
└─ tests.rs                           # status 快照测试

src/streaming/
├─ mod.rs                             # 流式状态/队列
├─ chunking.rs                        # chunking 策略
├─ commit_tick.rs                     # 流式提交 tick 计划
└─ controller.rs                      # StreamController/PlanStreamController

src/tui/
├─ event_stream.rs                    # crossterm 事件流 broker
├─ frame_rate_limiter.rs              # 帧率限制
├─ frame_requester.rs                 # 重绘请求调度
└─ job_control.rs                     # Ctrl-Z/suspend/resume 支持
```

## 测试与快照位置

```text
tests/
├─ all.rs                             # 集成测试总入口
├─ manager_dependency_regression.rs   # manager 依赖回归测试
├─ test_backend.rs                    # 集成测试 backend
├─ fixtures/oss-story.jsonl           # OSS 流程测试 fixture
└─ suite/
   ├─ mod.rs                          # suite 模块出口
   ├─ model_availability_nux.rs       # 模型可用性 NUX 测试
   ├─ no_panic_on_startup.rs          # 启动不 panic 测试
   ├─ status_indicator.rs             # 状态指示器测试
   ├─ vt100_history.rs                # VT100 历史渲染测试
   └─ vt100_live_commit.rs            # VT100 live commit 测试
```

快照资源分布：

```text
src/snapshots/                                      # app、diff_render、history_cell、markdown、model_migration、multi_agents、pager、resume、status_indicator、update_prompt 快照，共 85
src/bottom_pane/snapshots/                          # bottom pane 组件快照，共 114
src/bottom_pane/request_user_input/snapshots/       # request_user_input 快照，共 12
src/chatwidget/snapshots/                           # chatwidget 主测试快照，共 100
src/chatwidget/tests/snapshots/                     # chatwidget 子测试快照，共 4
src/onboarding/snapshots/                           # onboarding trust_directory 快照，共 1
src/render/snapshots/                               # render/highlight 快照，共 1
src/status/snapshots/                               # /status 快照，共 10
```

每个 `.snap` 文件职责均为对应文件名中测试用例的 insta 文本/VT100 快照基线；未逐个打开内容确认。

## 资源文件

```text
frames/
├─ blocks/frame_1.txt ... frame_36.txt    # blocks ASCII 动画帧
├─ codex/frame_1.txt ... frame_36.txt     # codex ASCII 动画帧
├─ default/frame_1.txt ... frame_36.txt   # default ASCII 动画帧
├─ dots/frame_1.txt ... frame_36.txt      # dots ASCII 动画帧
├─ hash/frame_1.txt ... frame_36.txt      # hash ASCII 动画帧
├─ hbars/frame_1.txt ... frame_36.txt     # horizontal bars ASCII 动画帧
├─ openai/frame_1.txt ... frame_36.txt    # openai ASCII 动画帧
├─ shapes/frame_1.txt ... frame_36.txt    # shapes ASCII 动画帧
├─ slug/frame_1.txt ... frame_36.txt      # slug ASCII 动画帧
└─ vbars/frame_1.txt ... frame_36.txt     # vertical bars ASCII 动画帧
```

## 模块组成摘要

- `src/lib.rs` 是库级装配入口，集中声明 TUI 的 app、chatwidget、bottom_pane、render、status、streaming、tui、onboarding 等模块。
- `src/main.rs` 是 `codex-tui` CLI 入口；`src/bin/md-events.rs` 是独立辅助二进制。
- `app*` 系列负责应用状态、app-server 会话、事件、审批转换和回退。
- `chatwidget*` 负责主聊天界面、历史渲染、实时状态、插件/skills/权限弹窗与大量 UI 快照测试。
- `bottom_pane/` 是输入区、footer、弹窗、审批、feedback、skills、request_user_input 的独立边界。
- `render/`、`markdown*`、`diff_render.rs`、`history_cell.rs` 是文本/Markdown/diff/历史单元渲染核心。
- `tui/` 和 `tui.rs` 是终端模式、事件流、帧调度、挂起恢复的底层 TUI 基础设施。
- `status/` 和 `status_indicator_widget.rs` 负责 `/status` 页面和紧凑状态指示器。
- `tests/` 是 crate 级集成测试；`src/**/snapshots` 是组件/单元级 insta 快照资源。

## 无法确认的项

- `prompt_for_init_command.md`、`styles.md`、`selection_list.rs`、`skills_helpers.rs`、`text_formatting.rs` 的职责有一部分来自文件名/位置推断，未做全文语义审计。
- `.snap` 文件数量多，已确认位置和命名用途，但未逐个读取快照正文。
- `frames/*/frame_*.txt` 可确认是 ASCII 动画帧资源；每一帧具体画面内容未逐帧审阅。
