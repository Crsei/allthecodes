先给结论：**Hermes Agent 的核心不是 TUI、不是模型、也不是工具数量，而是“长期运行的 Agent Runtime + 闭环学习系统”**。它把一次性 coding loop 扩展成一个能跨会话、跨平台、会记忆、会沉淀技能、能定时运行、能派生子代理的个人/团队级 agent 操作系统。

## 1. Hermes Agent 的核心是什么

可以用这个公式理解：

```text
Hermes = AIAgent Loop
       + Tool Registry / Toolsets / MCP / Sandboxes
       + SQLite SessionDB + FTS5 session_search
       + MEMORY.md / USER.md curated memory
       + Skills progressive disclosure + /learn
       + Background Review / Curator self-improvement loop
       + Gateway / Cron / Subagents / Multi-platform delivery
```

Hermes 官方 README 强调的卖点是“self-improving AI agent”：它会从经验里创建 skills、改进 skills、把知识写入 memory、搜索自己的历史会话，并支持 Telegram、Discord、Slack、WhatsApp、Signal、CLI 等入口共享同一套记忆与会话。([GitHub][1])

从架构文档看，Hermes 的核心路径是：**CLI/Gateway/ACP/Batch/API 等入口 → `AIAgent` → prompt builder / provider resolution / tool dispatch → SQLite+FTS5 session storage 与各类 tool backends**。这说明它不是“某个平台上的聊天机器人”，而是把平台入口和 agent core 分离了。

`AIAgent` 本身负责 prompt assembly、provider/API mode 选择、可中断模型调用、顺序/并发工具执行、会话历史、压缩、重试/fallback、迭代预算，以及在上下文丢失前 flush persistent memory。 这就是 Hermes 最关键的“心脏”。

它真正比普通 coding agent 强的地方有四个：

第一，**记忆不是简单向量库，而是 bounded curated memory + session search**。Hermes 用 `MEMORY.md` 和 `USER.md` 保存关键事实，并在 session 开始时冻结注入 prompt；所有历史会话进入 SQLite + FTS5，可按需搜索。

第二，**技能系统是渐进披露的知识层**。Skills 是按需加载的 `SKILL.md` 文档，先只暴露列表和描述，真正需要时再加载完整内容，避免 prompt 被大量技能污染。

第三，**闭环学习**。后台 self-improvement review 会在 turn 之后悄悄总结哪些东西应该写入 memory 或升级成 skill；开启 approval gate 后这些写入可以先 staged 再审批。

第四，**Gateway / Cron / Subagents 让 agent 长期存在**。Hermes 的 gateway 是长运行进程，负责多平台消息、用户授权、slash command、cron ticker；cron job 不是 shell task，而是 first-class agent task；subagents 则是隔离上下文的任务分派。

## 2. Codex 和 Hermes 的本质差异

Codex 现在更像一个**强 coding loop + 本地开发环境执行器**。OpenAI Codex repo 说明它是运行在本地的 coding agent，主项目也已经高度 Rust 化。([GitHub][2])

从 `codex-rs/core/src/session/turn.rs` 看，Codex 的核心 loop 已经很成熟：模型返回 function call 或 assistant message；如果是工具调用，就执行并把结果送回下一次 sampling；如果是 assistant message，就记录历史并结束 turn。

Codex 也已经有 skills/plugins/MCP/connectors 的注入机制，并且 `built_tools` 会从 MCP、plugins、connectors、dynamic tools 构建 `ToolRouter`。 Prompt 构造时也会把 `router.model_visible_specs()` 暴露给模型。

所以，**不要从“重写 agent loop”开始改 Codex**。Codex 缺的不是 loop，而是 Hermes 那套“长期运行、跨会话、可学习、可调度、可多入口”的 runtime 外壳。

## 3. 把 Codex 改造成 Hermes-like 的正确路线

我建议采用 **Codex core 不大改，外面加 Persistent Agent Runtime** 的方式。

### Phase 1：先加 SessionDB / Event Store

目标是把 Codex 从“当前 session 内可用”变成“所有会话可检索、可恢复、可 lineage”。

建议新增：

```text
codex-rs/agent-memory/
  db.rs              # SQLite + WAL
  schema.rs          # sessions/messages/messages_fts
  session_search.rs  # FTS5 + trigram/CJK search
  lineage.rs         # compact/resume lineage

codex-rs/core/src/session_persistence.rs
```

存储这些东西：

```sql
sessions(id, source, cwd, model, started_at, parent_session_id, title, token_usage)
messages(id, session_id, role, content, tool_calls, tool_name, reasoning, timestamp)
messages_fts(content, tool_name, tool_calls)
```

Hermes 的 session storage 参考价值很高：它用 SQLite/WAL 持久化 session metadata、完整消息历史、模型配置，并用 FTS5 / trigram tokenizer 支持会话搜索和 CJK/substring 检索。

对 Codex 来说，这一步应该接在 `run_turn` 生命周期旁边：每次 user input、assistant output、tool call、tool result、compact summary 都进入 event/session store。**不要让 TUI、Web、CLI 各存一份状态**，状态源只能有一个。

### Phase 2：加 Codex Memory Layer

先不要上复杂知识图谱，先做 Hermes 这种小而强的 curated memory：

```text
~/.codex/memories/
  MEMORY.md  # 环境、项目、工具经验、踩坑
  USER.md    # 用户偏好、沟通风格、长期约束
```

新增 agent-level tool：

```rust
memory(action: add | replace | remove, target: memory | user, content, old_text?)
```

设计要点：

```text
1. memory 有严格字符上限
2. session start 时冻结注入 prompt
3. session 中写入立即落盘，但不动态修改当前 system prompt
4. 写入可配置 approval gate
5. 记忆只存“长期有用事实”，不存临时日志
```

Hermes 明确采用 frozen snapshot pattern：memory 在 session 开始时注入，session 中写入会持久化，但不会破坏当前 prompt cache。 这点很适合 Codex，因为 Codex 的上下文和缓存稳定性也很重要。

### Phase 3：把 Skills 做成真正的可学习层

Codex 已经有 skill injection 的基础，但要拥有 Hermes 能力，需要把 skill 从“静态提示词”升级成“agent 可创建、可修改、可审批、可检索、可渐进披露”的系统。

建议目录：

```text
~/.codex/skills/
  coding/
    rust-debug/
      SKILL.md
      references/
      scripts/
  devops/
    nginx-deploy/
      SKILL.md
```

新增工具：

```text
skills_list()
skill_view(name, path?)
skill_manage(action=create|patch|replace|delete, name, content, diff?)
learn_skill(source: url|dir|conversation|notes)
```

Hermes 的 `/learn` 很关键：它能把本地目录、在线文档、刚刚走过的流程、粘贴的操作说明转成可复用 skill，并通过 `skill_manage` 保存。

对 Codex 改造时，`/learn` 可以特别针对 coding 场景：

```text
/learn how we fixed the daemon reconnect bug
/learn this repo's release workflow
/learn how to debug MCP startup timeout in this project
/learn the allthecodes bridge architecture
```

这样 Codex 就不是每次重新理解你的项目，而是逐渐沉淀“项目级操作手册”。

### Phase 4：加 Background Review，这是 Hermes 的灵魂

这是最值得抄思想的地方。

每 N 个 turn，或每次任务完成后，启动一个低成本 auxiliary model review：

```text
输入：
- 最近若干 turn
- tool calls / errors / user corrections
- changed files summary
- tests / verification result
- current MEMORY.md / USER.md / relevant skills

输出：
- 是否要新增 memory
- 是否要替换旧 memory
- 是否要创建 skill
- 是否要 patch skill
- 是否发现 workflow pitfall
```

结果不要直接乱写，进入 pending queue：

```text
/codex memory pending
/codex memory approve <id>
/codex skills pending
/codex skills diff <id>
/codex skills approve <id>
```

Hermes 的 background review 就是把重复纠正、长期工作流经验变成 memory 或 procedural skills，并且可以通过 approval gate 审批。

这一步做完，Codex 才真正拥有 Hermes 的“越用越懂你、越用越懂项目”的能力。

### Phase 5：做 Gateway，但不要一上来支持 20 个平台

Hermes 支持很多平台，但你改 Codex 时不应该直接复制数量。先做三层：

```text
codex-agentd
  - WebSocket / JSON-RPC event stream
  - session routing
  - auth / pairing
  - cron ticker
  - background review worker
  - task registry

frontends
  - TUI
  - Web dashboard
  - Telegram/Slack 二选一
```

入口统一走：

```text
User message
  -> Gateway SessionRouter
  -> Codex Session / run_turn
  -> Tool events / assistant events
  -> SessionDB
  -> Delivery adapter
```

这样你的 allthecodes / Codex App / Web UI / Telegram 都只是 adapter，不要各自实现 agent logic。

### Phase 6：把 Subagents 做成 coding worktree workers

Hermes 有 `delegate_task` 子代理，但 Codex 的强项是 coding，所以你可以做得更进一步：

```text
delegate_task(
  role = "implementer" | "reviewer" | "tester" | "security" | "docs",
  repo,
  worktree,
  prompt,
  budget,
  allowed_tools,
  verification_policy
)
```

每个子代理绑定：

```text
独立 session_id
独立 worktree/container
独立 tool budget
独立 memory view
独立 diff
父任务可 wait/resume/interrupt/merge
```

你的目标可以是：

```text
Plan Agent 负责拆任务
Codex Implementer 负责改代码
Claude/Codex Reviewer 负责 review diff
Test Agent 负责跑测试和定位失败
Release Agent 负责 changelog / commit / PR
```

这比 Hermes 更贴近 coding agent 的未来形态。

## 4. 和 Codex 现有代码的映射

可以这样落地：

```text
Codex 现有能力                    Hermes-like 增强
------------------------------------------------------------
run_turn loop                     保留，不重写
ToolRouter / built_tools           扩展 memory/skill/session_search/delegate_task
hooks / after-agent hook           接 background review trigger
compact                           接 session lineage + memory flush
skills injection                  改成 progressive disclosure + skill_manage
MCP/connectors                    加 trust boundary / capability approval
TUI session                       改成 event-stream consumer
```

最重要的原则是：**Codex 的工具执行路径必须保持唯一**。不要为了 gateway、cron、subagent 又搞一套 parallel tool executor。所有入口最终都应该落到同一个 `run_turn` / `ToolRouter` / execution path。

## 5. 更进一步：不要只做 Hermes clone，要做 Coding Agent Runtime

Hermes 是泛用个人 agent，Codex 是 coding agent。你真正应该做的是：

```text
Hermes-like Persistent Runtime
+ Codex-level code execution
+ Worktree/subagent orchestration
+ Verification-first workflow
+ Skill/memory self-improvement
= Coding Agent OS
```

更进一步可以做这几件事：

第一，**Verification Loop 优先于“更聪明的 memory”**。每个 coding task 都必须有：

```text
plan -> patch -> test -> review -> fix -> commit/PR
```

每个 skill 也要带 verification section，不然 skill 会变成“会污染未来行为的幻觉文档”。

第二，**Memory 分层**：

```text
Hot memory: USER.md / MEMORY.md，少量高价值事实
Warm memory: SQLite FTS session_search
Cold memory: repo docs / AGENTS.md / plans / old PRs
Semantic memory: 最后再加 Honcho/Mem0/knowledge graph
```

Hermes 也把 built-in memory 和外部 memory providers 区分开，外部 provider 只是增强 semantic search、knowledge graph、user modeling，不是替代内置 memory。

第三，**技能要有生命周期管理**。Hermes 的 curator 会追踪 agent-created skills 的使用、过期、归档、合并，避免 self-improvement 产生一堆窄而重复的 skills。 你的 Codex fork 也应该有：

```text
active -> stale -> archived
pin skill
rollback skill
skill usage telemetry
skill diff approval
```

第四，**安全边界要比 Hermes 更强**。最近已经出现过伪装成 Codex Web UI 的 npm 包窃取认证 token 的供应链攻击，也出现过 Codex 命令注入相关研究与修复；所以给 Codex 加 gateway、插件、MCP、自动安装能力时，必须先做 trust boundary、签名/来源校验、危险命令审批、secret scope 和 sandbox policy。([TechRadar][3])

## 6. 我建议你的最小可行改造顺序

不要先做 20 个平台，也不要先做知识图谱。最优先顺序是：

```text
1. SessionDB + FTS5 + session_search
2. MEMORY.md / USER.md + memory tool
3. skills_list / skill_view / skill_manage + progressive disclosure
4. /learn from conversation / repo docs
5. after-turn background review + pending approval
6. agentd event stream + Web/TUI adapter
7. cron job + one messaging platform
8. delegate_task + worktree subagents
9. curator + skill lifecycle
10. evaluation / trajectory / self-training data
```

一句话总结：**Hermes 的核心是“让 agent 长期活着并自我沉淀”；Codex 的核心是“强 coding loop”。你要做的不是把 Hermes 代码塞进 Codex，而是给 Codex 套上一层 persistent runtime、memory/skills/self-review/gateway/subagent 系统。这样会比 Hermes 更适合 coding，也更接近你想做的 allthecodes。**

[1]: https://github.com/nousresearch/hermes-agent "GitHub - NousResearch/hermes-agent: The agent that grows with you · GitHub"
[2]: https://github.com/openai/codex "GitHub - openai/codex: Lightweight coding agent that runs in your terminal · GitHub"
[3]: https://www.techradar.com/pro/security/openai-codex-tool-with-over-29-000-downloads-linked-to-malicious-npm-supply-chain-attack-stealing-authentication-tokens?utm_source=chatgpt.com "OpenAI Codex tool with over 29,000 downloads linked to malicious npm supply chain attack stealing authentication tokens"
