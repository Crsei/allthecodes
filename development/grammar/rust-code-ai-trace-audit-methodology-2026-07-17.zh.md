# Rust 代码 AI 痕迹审计方法（并行子代理拆分法）

> 日期：2026-07-17
> 来源：对 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/grok-build`（SpaceXAI grok CLI 的对外透明端口，约 2,600 个 `.rs` 文件 / 74 个 crate）的实测审计
> 适用：判断一个 Rust 代码库是 senior 人类编写还是 LLM 主导编写；定位局部 AI 辅助残留
> 输出形态：1-10 评分（1 = 纯人类 idiomatic，10 = 公然 LLM 生成）+ 每信号 verdict + file:line 证据

## 1. 方法论核心

**单上下文装不下大型 Rust workspace 的逐文件审计；用并行只读 Explore 子代理按"维度"切分，每个子代理在一个独立 context 内只用 MCP boost 工具读片段、做计数 + 抽样，最后由主代理汇总裁决。**

切分维度不是按目录，而是按"AI 痕迹在不同侧面会暴露的不同特征"——这样四个子代理之间无重叠、无彼此依赖，可纯并行。

四个维度：

| 子代理 | 维度 | 覆盖的 AI 信号 |
|---|---|---|
| 1 | 错误处理与 panic 风格 | unwrap 纪律 / `?` vs 显式 match / thiserror vs 手写 / 错误信息是否泛化 / anyhow 是否泄漏进纯库 crate / catch-all |
| 2 | 命名 / 注释 / 文档 / 模块组织 | doc 注释是否带 `# Panics`/`# Errors`/`# Examples` 段 / `//!` 头是否套话 / `get_`/`process_`/`handle_` 命名 / TODO/FIXME 残骸 / 过度抽象 / `#![allow]` 堆叠 |
| 3 | 类型系统 / trait / unsafe / 并发 | 命名生命周期是否只在真需要处 / 泛型 bound 是否厨房水槽 / trait 是否多 impl / `unsafe` 是否带 `// SAFETY:` / 原子 vs `Mutex<Bool>` 选择 / `await` 持锁 / derive 厨房 |
| 4 | 测试 / 依赖 / 构建 / lint / prose | 测试深度与命名 / snapshot/property/fuzz / **`Co-Authored-By`/`Generated with`/`🤖` 直接指纹 grep** / lint 立场 / `.cargo/config.toml` 加固 / toolchain pinning / changelog 散文 / README/CONTRIBUTING/SECURITY 格调 |

## 2. 关键：直接指纹 grep 是判读基石

四个维度中，**子代理 4 内的「`Co-Authored-By` / `Generated with [Aa]nthropic` / `🤖 Generated with` / `noreply@anthropic\.com` / `written by Claude|GPT` / `machine-generated`」全文 grep 是最不可能误判的判定**。在做其它间接信号分析前，先跑这一道：

```
search_grep pattern="Co-Authored-By" glob="*.{rs,md,toml,txt,json,yaml,yml}"
search_grep pattern="Generated with"   glob="*.{rs,md,toml,txt,json,yaml,yml}"
search_grep pattern="🤖"                glob="*.{rs,md,toml,txt,json,yaml,yml}"
```

注意排除误命中：

- `claude` 字样常出现在**功能名**而非署名（如 Claude Code session 互操作性、import_claude.rs）。要逐条核对，不能机械计数。
- `LLM-generated`/`AI-generated` 在 senior 代码中常出现于**描述运行时输出**（如 compaction summary、AI-suggested shell command 的注释），不代表作者身份。
- changelog 中的 `Claude/Cursor/Codex` 通常是兼容性扫描功能，不是 trailer。

**零命中 ≠ 必然纯人类**（agent 可能迭代后清掉 trailer），**但零命中 + 大量 senior 间接信号 = 强人类证据**；**有命中 = 几乎确定有 AI 辅助**，逐条读上下文定性。

## 3. 子代理调度要点

### 3.1 工具与根目录

boost MCP 服务器根目录是上一级 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao`，所以 `fs_read` / `search_grep` / `fs_scan` / `code_outline` 调用必须**始终带 `grok-build/` 前缀**。子代理 prompt 里要**显式写明**这一条，否则会读到错误目录或返回空。

```
mcp__boost__daemon_status  # 先 root 字段确认根目录
```

### 3.2 每个 prompt 必含 6 件事

1. **明确的"维度隔离"声明**：开头就写明本子代理只看某一类信号，不重复其它子代理的工作（避免他们读同一批文件浪费 token）。
2. **MCP 根前缀提醒**：所有路径加 `grok-build/`（或对应 workspace 名）。
3. **具体信号清单 + 每信号至少 3 个 file:line 证据要求**：不给具体清单子代理会泛泛而过。
4. **明确要 search_grep 的模式**：直接给出 regex / glob，节省子代理探索时间（如 `\\bunwrap\\(\\)`、`fn get_`、`\\bunsafe\\b`、`// SAFETY:`、`#\\[derive\\(Debug,`）。
5. **判定轴**：每信号出 `HUMAN-typical` / `AI-typical` / `MIXED` / `INCONCLUSIVE` 四档之一，并给 1-10 评分与一段总结。
6. **节流要求**：每个子代理输出限 ~500 行、verbatim 引用要短，避免回传主代理时 context 爆炸。

### 3.3 并行还是串行

四维彼此无依赖 → **单条消息内 4 个 Agent 调用并发**，由 harness 跑。完成通知陆续回来，主代理不阻塞。

### 3.4 子代理类型选 `Explore` 不是 `general-purpose`

`Explore` 是只读研究代理，没有 Edit/Write，不会误改仓库；适合审计场景。

## 4. 各维度的具体可执行信号清单（直接复用）

### 维度 1：错误处理

- 计数：`\bunwrap\(\)`、`\bexpect\(`、`unwrap_or_else`、`?`（行内 `\?` 出现次数）、`match .*Err\(`、`panic!`、`unreachable!`、`unimplemented!`、`todo!`、`_ =>` catch-all。
- 抽样 15-20 个 `unwrap()` 命中分类：是 `Mutex::lock().unwrap()`、sentinel-fallback after `is_some()`、infallible stdlib on controlled bytes，还是裸用？
- `expect("…")` 信息是否短而具体（`"current_prompt_id mutex poisoned"`、`"IndexManager dropped before responding"`）？泛化（`"Operation succeeded"`）是 LLM tell。
- `#[derive(thiserror::Error)]` enum 的 `#[error("…")]` 是否带字段插值、破折号、inline format expr？
- `anyhow::Result` 是否只在边界 crate（CLI、main、跨平台子进程封装）出现，**不**渗入纯协议/类型 crate？
- `.context()`/`.with_context()` 出现次数（senior 信号，LLM 默认裸 `?`）。
- 是否有 `match Ok(x) => …, Err(e) => …` 两臂样板可直接换 `?` 的（LLM smell）。

### 维度 2：命名/注释

- `fn get_` / `fn set_` / `fn process_` / `fn handle_` / `fn do_` 计数（vs 总 `fn `）。`get_` 在 idiomatic Rust 中近乎消失，>10 即可疑。
- `//!` 模块头是否套话："This module provides a unified interface for X, supporting A, B, and C" 是经典 LLM tell。聚焦 grep `^\s*//\s*This module (provides|contains|defines)` 验证。
- 公共项 doc 是否带 `# Panics` / `# Errors` / `# Examples` 段（senior 信号，LLM 缺席）。
- `// TODO`、`// FIXME`、`// HACK` 是否带具体语境、半句悬挂、typo（如 `looked like,,`）—— 真人 TODO 杂乱，LLM 清洁化后不留这类残骸。
- 非英文注释：`[^\\x00-\\x7F]` 行级 grep，但要剔除 ASCII box-rule 字符 `─`（U+2500）等装饰。
- `trait` 定义 vs `impl` 块比例；spot-check 3 个 trait，看是单 impl 的过度抽象（mock-driven 是合理的）还是真多 impl。
- `*Manager` / `*Handler` / `*Service` 计数；判断是否每概念一 Manager（LLM overgeneralization smell）。
- crate 顶层 `#![allow(...)]` 是否 blanket silence `dead_code` / `unreachable_code`（迭代 AI patch 的典型残留）。

### 维度 3：类型系统/unsafe/并发

- 命名生命周期 `<'a>`、`<'_`、`'static` 计数 + 抽样 12-15 处，归类"真需要 vs 机械贴"。`Formatter<'_>` 是 rustfmt canonical，不算 smell。
- 最深泛型签名：3+ 类型参数、`where` 子句、厨房水槽 bound（`T: Clone + Send + Sync + 'static + A + B + C`）。
- trait 是否用 associated types 优于泛型参数；是否用 RPITIT 原生 async in trait + 显式 `Send` bound 而非 `#[async_trait]` boxing；是否 `Tool` typed + `ToolDyn` erased + blanket bridge。
- `unsafe` 用 `\\bunsafe\\b` grep（排除 vendored `third_party/`）；每个块是否有 `// SAFETY:` 注释带不变量。
- 5 个以上 crate 是否 `#![forbid(unsafe_code)]`（强 senior 信号）。
- `Arc<` / `Mutex<` / `RwLock<` / `parking_lot::` / `OnceLock` / `AtomicBool` / `AtomicUsize` 计数。是否 atomics 用于热读路径、`Mutex` 仅给多字段状态、有无 `Mutex<AtomicBool>` 这种侮辱。
- `.lock().await` 后续是否还有 `.await`（持锁跨 await 是中级 LLM 常见 leakage）。
- `struct .*Builder` 计数；每个 builder 是否真有可选字段 + terminal `.build()`，还是 ceremony。
- `pub fn new(` 返回是否一致 `Self` vs `Result<Self, _>`；不一致的兄弟 crate 是部分 LLM 信号。
- `#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]` 厨房水槽是否出现（典型 LLM 8-derive 签名 → 强 tell）。

### 维度 4：测试/依赖/构建/lint/prose

- `#[test]` vs `#[tokio::test]` 比例。senior 大致 14% async；LLM 倾向一律贴 `#[tokio::test]`。
- 测试 fn 命名：行为式 `does_not_bleed_assistant_into_next_turn`（senior）vs 模板 `happy_path`/`edge_case`/`it_works`（LLM）。模板名总数 / 总测试数 < 0.1% 是强人类信号。
- `insta::` / `expect_test` / `proptest` / `arbitrary` / `quickcheck` / `#[fuzz]` 出现位置与广度。snapshot/property test 存在 = 强人类信号（LLM 维护库基本不主动用）。
- `[features]` 是否 deliberate：`jemalloc = []` no-op 是否带 build-system rationale；`test-hooks` 是否是跨 crate 测试 escape hatch；`required-features` 是否按 scheduling family 拆 `[[test]]`（CI 矩阵工程化）。
- `[workspace.lints.clippy]` / `clippy.toml disallowed-methods` 是否带 per-lint 引用理由的 allowlist（senior）；是否每条 `allow` 都引用上游 issue / Bazel lint aspect 同步（强 release-engineering 信号）。
- `.cargo/config.toml` 是否带 per-target RELRO/NX/noexecstack linker 加固 + jemalloc page-size 按 arch 调（LG_PAGE=14 for Apple Silicon 16KB / LG_PAGE=16 for Linux 64KB），并引用上游 issue（`tikv-jemallocator#122`、`pyo3.rs`、`backtrace-rs#397`）。这种细节 LLM 编不出来。
- `rust-toolchain.toml` 是否有 channel + bump 政策散文 + components + targets + `# force CI` trailer。
- changelog 是否带真实日期、具体数字（"10-hour default timeout"、"byte budgets"）的散文，零 AI trailer。
- `[workspace.dependencies]` 是否单一 workspace-shared 版本 per dep（无重复版本），所有 crate 用 `{ workspace = true }` + 加性 features。
- `[profile.release-dist / x-prod / release-dist-jemalloc]` 分层 + dev `panic = abort` + `split-debuginfo` 调优。
- `README.md` 是否露出"internal monorepo sync"等真信息；`CONTRIBUTING.md` 是否 brusque（"No CLA because external contributions are not accepted"）；`SECURITY.md` 是否给真实 HackerOne URL。模板型 boilerplate 是 LLM tell。

## 5. 判读的"反方向"陷阱

间接信号会同时给彼此矛盾的证据，主代理汇总时要警惕：

| 看似 AI 痕迹 | 实则常是 senior 人类代码 |
|---|---|
| 几千个 `unwrap()` 抽样后大半是裸用 | 抽样后大半是 `Mutex::lock` / sentinel-fallback / infallible-stdlib → 是人类 |
| 有 `unreachable!()` | 22/39 带消息、相邻 arm 分三条不同消息区分异步路径 → 是人类 |
| 有 anyhow 在 src/ | 限定在跨平台子进程封装 `clipboard.rs`、CLI 边界 → 是人类；渗入纯协议 crate 才是 smell |
| `#![allow]` 多达 185 处 | 每条 per-site strategic（`arc_with_non_send_sync` 在 `tokio::sync::Notify` 含类型上 lint 是错的）→ 是人类；crate 顶 blanket silence `dead_code` 才是 LLM 残留 |
| 没用 proptest | snapshot/insta/fuzz 至少有一个，且 `proptest` 是不存在而非"代码有但浅" → 弱信号，而非 AI 痕迹 |
| `PartialOrd, Ord` derive 偏多 | 经典 LLM 8-derive 厨房签名 0 次、只有零星 `Ord` 几处 → 轻微 cargo-cult，不是 AI 主写 |
| "unknown error" 字符串命中 | 多数是 `unwrap_or_else(\|\| "unknown error".to_string())` 的用户面板回退、或测试 fixture → 不是 panic 信息 |

**反之**：当直接的 trailer grep 零命中、间接信号几乎全 senior 时，唯一还能采到的局部 AI 辅助残留通常是：

- 个别 `lib.rs` 顶部 `#![allow(unused_imports, unused_variables, unused_mut, unreachable_code, dead_code)]` blanket + 该 `lib.rs` 缺 `//!` 模块头（同一文件两件同时发生更可疑，是 「agent 反复 patch + 静音警告」 的工作流残留）。
- 零星 3-5 处 `//! "This module provides…"` 套话（聚焦 grep 区分 `//!` vs `//`：后者经典 LLM tell 在本仓库零命中）。
- 一两处冗余 `.map_err(\|e\| anyhow::anyhow!(e))`。

这些**不**改变整体判读（仍能稳在 2/10），但作为"AI 辅助痕迹"的最具体佐证应单列。

## 6. 输出格式约定

主代理汇总报告建议固定结构：

```
# <repo> Rust 代码 AI 痕迹审计综合报告

## 综合评分表
| 维度 | 子代理结论 | 评分 |

## 一、AI 痕迹直接指纹搜索（决定性反证或顺证）

## 二、强人类信号（LLM 难以伪造）

## 三、能采到的"AI 辅助"残留（局部、非整体）

## 四、整体判读

## 五、最终结论
```

每个子代理的输出回传主代理后，主代理不复述全部细节，只取**每信号 1-2 条最锐利的 file:line 证据 + 一句 verdict** 入表。子代理 token 数已耗在搜索上，主代理不要把它们再 grep 一遍。

## 7. 成本与边界

- 单次 4 子代理实测：4 × ~50 tool_uses，~400-700s/agent，主代理最终汇总 context 约 12k。
- workspace > 5,000 文件时考虑把维度 4 拆成两个子代理（测试/lint 一组、依赖/构建/prose 一组）避免单代理超时。
- vendored `third_party/` 一律排除 —— 它是上游人类 fork 的代码，混入会稀释判读信号。
- 审计 scope 仅写 `.rs` / `.toml` / `.md` / `.lock` 时要显式列 glob，避免命中 `target/` 旧构建产物。

## 8. 实测结果摘要（grok-build 2026-07）

| 维度 | Verdict | Score |
|---|---|---|
| 错误处理与 panic 风格 | HUMAN-typical | 2/10 |
| 类型系统 / trait / unsafe / 并发 | HUMAN-typical | 2/10 |
| 命名 / 注释 / 模块组织 | MIXED（局部 AI 辅助） | 3/10 |
| 测试 / 依赖 / 构建 / lint / prose | HUMAN-typical | 2/10 |

整体 ≈ **2/10**：senior 人类主导；唯一局部残留是 `crates/codegen/xai-grok-shell/src/lib.rs:1-7` 的 crate 顶 `#![allow(dead_code, unreachable_code, …)]` blanket silence + 该 `lib.rs` 缺 `//!` 模块头，以及 3-5 处 `//! "This module provides…"` 套话（`xai-file-utils/src/gcs.rs`、`xai-grok-pager-render/src/render/scrollbar.rs`、`xai-grok-shell/src/agent/proxy.rs`），1 处冗余 `.map_err(|e| anyhow::anyhow!(e))`（`xai-grok-shell/src/extensions/session_search.rs:87`）。直接 trailer grep 全工作区零命中。

## 9. 复用要点

把这份方法迁到其它 Rust workspace（如 `allthecodes/rust/`）只需：

1. 调整 MCP 根前缀（`daemon_status` 看 root，路径加对应 workspace 段）。
2. 把 prompt 里所有 `grok-build/` 路径前缀替换为新仓库根段。
3. 排除 vendored 上游 fork 目录（grok-build 是 `third_party/`，allthecodes 可能是其它）。
4. 重新跑 4 个并行 Explore 子代理，按本节信号清单逐条采样。
5. 主代理用同一输出格式汇总。
