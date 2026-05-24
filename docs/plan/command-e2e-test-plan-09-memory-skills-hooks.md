# 命令 E2E 测试计划 09：记忆、技能与钩子命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_memory_skills_hooks.rs`
> 所有测试均为**离线**。

skill必须测试初始化skill保存后agent能读到
memory同理
测试每个hook

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 备注 |
|---|---|---|---|
| `/memory` | `/mem`、`/global-search`、`/quick-open` | `Output` | 显示 CLAUDE.md 内容（默认） |
| `/memory show` | -- | `Output` | 显示 CLAUDE.md 摘要 |
| `/memory path` | -- | `Output` | 列出 CLAUDE.md 路径 |
| `/memory edit` | -- | `Output` | 创建/定位 CLAUDE.md |
| `/memory list` | `/memory ls` | `Output` | 列出记忆目录项 |
| `/memory get <key>` | -- | `Output` | 获取记忆项 |
| `/memory set <key> <value>` | -- | `Output` | 设置记忆项 |
| `/memory rm <key>` | -- | `Output` | 移除记忆项 |
| `/memory search <query>` | -- | `Output` | 搜索记忆 |
| `/skills` | -- | `Output` | 列出已加载的技能 |
| `/skills list` | -- | `Output` | 列出技能 |
| `/skills <name>` | -- | `Output` | 技能详情 |
| `/hooks` | -- | `Output` | 合并后的钩子树 |
| `/hooks list` | -- | `Output` | 列出钩子 |
| `/hooks list <event>` | -- | `Output` | 按事件筛选 |
| `/hooks path <scope>` | -- | `Output` | 设置文件路径 |
| `/hooks open <scope>` | -- | `Output` | 在编辑器中打开 |

## 测试用例

### T01：`/memory` 显示 CLAUDE.md 内容
```rust
fn memory_default_shows_claude_md() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("memory")
    // 3. Wait(2s)
    // 4. AssertScreenContains("CLAUDE") 或 AssertScreenContains("memory")
    // 5. Snapshot("memory_default")
    // 断言：显示 CLAUDE.md 内容或 "no CLAUDE.md found"
}
```

### T02：`/memory path`
```rust
fn memory_path() {
    // 步骤：
    // 1. Command("memory path")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 CLAUDE.md 文件路径
}
```

### T03：`/memory edit`
```rust
fn memory_edit() {
    // 步骤：
    // 1. Command("memory edit")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：创建或定位 CLAUDE.md
}
```

### T04：`/memory list`
```rust
fn memory_list() {
    // 步骤：
    // 1. Command("memory list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出记忆项或显示 "no items"
}
```

### T05：`/memory set` 和 `get` 和 `rm`
```rust
fn memory_set_get_rm_cycle() {
    // 步骤：
    // 1. Command("memory set e2e_test_key test_value")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("memory get e2e_test_key")
    // 5. Wait(2s)
    // 6. AssertScreenContains("test_value")
    // 7. Command("memory rm e2e_test_key")
    // 8. Wait(2s)
    // 9. AssertNoPanic
    // 断言：完整的 CRUD 循环可用
}
```

### T06：`/memory set` 带 `--global`
```rust
fn memory_set_global() {
    // 步骤：
    // 1. Command("memory set e2e_global_key global_val --global")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：全局记忆已设置
}
```

### T07：`/memory set` 带 `--category`
```rust
fn memory_set_category() {
    // 步骤：
    // 1. Command("memory set e2e_cat_key cat_val --category=test")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：分类记忆已设置
}
```

### T08：`/memory search`
```rust
fn memory_search() {
    // 步骤：
    // 1. Command("memory search e2e_test")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：搜索执行无 panic
}
```

### T09：`/memory` 别名 `/mem`
```rust
fn memory_alias_mem() {
    // 步骤：
    // 1. Command("mem")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T10：`/skills` 列出技能
```rust
fn skills_list() {
    // 步骤：
    // 1. Command("skills")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Snapshot("skills_list")
    // 断言：列出可用技能
}
```

### T11：`/skills list`
```rust
fn skills_list_subcommand() {
    // 步骤：
    // 1. Command("skills list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出技能
}
```

### T12：`/skills <name>` 详情
```rust
fn skills_detail() {
    // 步骤：
    // 1. Command("skills review")  // 或任意已知技能名称
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示技能详情或 "not found"
}
```

### T13：`/hooks` 显示钩子树
```rust
fn hooks_tree() {
    // 步骤：
    // 1. Command("hooks")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Snapshot("hooks_tree")
    // 断言：显示合并后的钩子树
}
```

### T14：`/hooks list`
```rust
fn hooks_list() {
    // 步骤：
    // 1. Command("hooks list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出钩子
}
```

### T15：`/hooks list <event>`
```rust
fn hooks_list_filtered() {
    // 步骤：
    // 1. Command("hooks list PreToolUse")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：筛选后的钩子列表
}
```

### T16：`/hooks path <scope>`
```rust
fn hooks_path() {
    // 步骤：
    // 1. Command("hooks path project")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示设置文件路径
}
```

## 优先级：高
记忆和技能是核心功能。CRUD 循环测试（T05）尤为重要。

## 补充功能说明

### `/memory`（别名 `/mem`、`/global-search`、`/quick-open`）

- **功能描述**：记忆管理入口命令。无参数时显示 selector（分组列出所有 scope 的记忆条目 + auto-memory 状态 + CLAUDE.md 文件 + 目录快捷方式）。后续子命令覆盖 CLAUDE.md 展示/编辑和 memdir CRUD 操作。
- **输出示例（无参数 selector）**：
  ```
  **Memory selector**
  Auto-memory: OFF (toggle via /memory auto on)

  CLAUDE.md files (1):
    /home/user/project/CLAUDE.md

  [1] [global] my_key - 2026-04-20
  [2] [project] auth_notes - 2026-04-18

  Directory shortcuts:
    [a] auto-memory dir    - /home/user/.cc-rust/auto-memory
    [t] team-memory dir    - /home/user/.cc-rust/team-memory
    [g] global memory dir  - /home/user/.cc-rust/memory
    [p] project memory dir - /home/user/project/.cc-rust/memory

  Open a directory with `/memory open <auto|team|global|project>`.
  ```
- **输出示例（无记忆条目时）**：
  ```
  (no memory entries; use `/memory set <key> <value>` to create one)
  ```
- **源码路径**：
  - 主处理器：`crates/cc-commands/src/memory.rs`（`MemoryHandler`）
  - Selector：`crates/cc-commands/src/memory/selector.rs`
  - CLAUDE.md 子命令：`crates/cc-commands/src/memory/claude_md.rs`
- **边界情况**：
  - 未知子命令：显示完整用法提示（含所有可用子命令说明）
  - 无记忆条目：selector 显示 "(no memory entries)"
  - CLAUDE.md 不存在：selector 不显示文件列表段
  - `get`/`set`/`rm`/`search` 缺少必需参数时：显示对应用法
  - `set` 值中带 `--global`/`--team`/`--auto` 标志：会被解析为 scope 而非值
  - `rm` 不带 `--global` 等标志默认操作 project scope
  - 搜索不到结果时："No memories matching 'xxx'."
  - 未传参数的子命令（如 `/memory set`）：显示用法而非 panic

### `/memory show`

- **功能描述**：显示当前工作目录及父目录层级中找到的 CLAUDE.md 文件内容。使用 `build_claude_md_context` 聚合所有层级的指令文件。
- **输出示例（有文件）**：
  ```
  **Project Instructions (CLAUDE.md)**

  # AGENTS.md - cc-rust
  ...
  ```
- **输出示例（无文件）**：
  ```
  No CLAUDE.md found in project hierarchy.

  Use `/memory edit` to create one.
  ```
- **边界情况**：
  - 多个父目录均含 CLAUDE.md：合并展示所有文件内容
  - 文件内容为空：返回空的 context，仍显示 header

### `/memory path`

- **功能描述**：列出所有找到的 CLAUDE.md 文件路径及其大小。
- **输出示例**：
  ```
  CLAUDE.md files found:
    /home/user/project/CLAUDE.md (1234 bytes)
    /home/user/CLAUDE.md (567 bytes)
  ```
- **输出示例（无文件）**：
  ```
  No CLAUDE.md files found.
  ```
- **边界情况**：
  - 文件不存在：不显示
  - 无文件时输出清晰提示

### `/memory edit`

- **功能描述**：在当前工作目录下创建或定位 CLAUDE.md。若不存在则创建带模板的文件；若已存在则仅返回路径。
- **输出示例**：
  ```
  CLAUDE.md location: /home/user/project/CLAUDE.md

  Edit this file to add project instructions.
  ```
- **边界情况**：
  - 文件已存在：不会覆盖，只输出路径
  - 目录无写入权限：触发错误（`fs::write` 返回 Err）

### `/memory list`（别名 `/memory ls`）

- **功能描述**：列出所有 scope 的 memdir 记忆条目，按 scope 分组显示。Team 记忆仅在 `FEATURE_TEAM_MEMORY=1` 或已有数据时显示；Auto 记忆仅在 `autoMemoryEnabled=true` 或已有数据时显示。
- **输出示例**：
  ```
  **Project memories** (2)
    auth_notes - use OAuth2 flow [auth]
    style - use rustfmt [code]

  **Global memories** (1)
    api_base - https://api.example.com
  ```
- **输出示例（无记忆）**：
  ```
  No memory entries found.

  Use `/memory set <key> <value>` to create one.
  ```
- **边界情况**：
  - 全部 scope 均无条目：显示 "No memory entries found" 提示
  - 值超过 60 字符：截断显示 "..."

### `/memory get <key>`

- **功能描述**：按 key 读取记忆条目，搜索顺序为 project → global → team → auto，首次命中即返回。
- **输出示例**：
  ```
  **my_key** (global)
  my_value
  Category: code
  Created:  2026-04-20T10:00:00Z
  Updated:  2026-04-21T15:30:00Z
  ```
- **输出示例（未找到）**：
  ```
  Memory 'missing_key' not found.
  ```
- **边界情况**：
  - key 为空：显示用法
  - 多个 scope 有同名 key：返回优先级最高的（project > global > team > auto）
  - 无 category 时不显示 Category 行

### `/memory set <key> <value> [--global|--team|--auto] [--category=<cat>]`

- **功能描述**：写入或更新记忆条目。默认写入 project scope；通过标志可切换到 global/team/auto scope。可指定 category 分类。
- **输出示例**：
  ```
  Saved global memory 'api_base': https://api.example.com
  ```
- **输出示例（带 category）**：
  ```
  Saved project memory 'style': use_rustfmt
  ```
- **边界情况**：
  - 缺少 key 或 value：显示用法
  - value 中混合标志和内容：标志被过滤，剩余部分拼接为 value
  - 重复 key 写入：覆盖（memdir::write_memory 处理）
  - `--global` 同时出现在 value 和 rm flag 位置

### `/memory rm`（别名 `/memory delete`、`/memory del`）

- **功能描述**：删除指定 scope 的记忆条目。默认删除 project scope；可用 `--global`/`--team`/`--auto` 切换。
- **输出示例（删除成功）**：
  ```
  Deleted project memory 'e2e_test_key'.
  ```
- **输出示例（未找到）**：
  ```
  Memory 'e2e_test_key' not found in project scope.
  ```
- **边界情况**：
  - key 为空：显示用法
  - key 不存在：提示 not found，不 panic
  - 无 `--global` 标志时默认 project scope

### `/memory search`（别名 `/memory find`）

- **功能描述**：在所有 scope 中进行子字符串匹配搜索，返回匹配的条目。
- **输出示例（有结果）**：
  ```
  Found 2 result(s) for 'rustfmt':
    [project] style - use_rustfmt [code]
    [global] formatter - rustfmt
  ```
- **输出示例（无结果）**：
  ```
  No memories matching 'nonexistent'.
  ```
- **边界情况**：
  - 查询为空：显示用法
  - 多词查询：作为整体子串匹配
  - 值超过 60 字符：截断显示

### `/memory auto on|off|status`

- **功能描述**：切换 auto-memory 捕获功能。`on`/`off` 同时持久化到 settings 文件并更新运行时状态；`status` 显示当前开关。当前仅持久化设置，auto-capture hook 尚未接入。
- **输出示例（on）**：
  ```
  Auto-memory: ON
  Persisted to /home/user/.cc-rust/settings.json

  Note: the auto-capture hook is not yet wired; this toggle persists the setting only.
  ```
- **输出示例（status）**：
  ```
  Auto-memory: OFF (setting key: autoMemoryEnabled)
  ```
- **边界情况**：
  - settings 文件不存在：创建新文件
  - settings 文件内容无效 JSON：报错，不会覆盖
  - 未知 action：显示用法

### `/memory open <auto|team|global|project>`

- **功能描述**：确保指定 scope 的记忆目录存在并输出其路径。不直接打开编辑器，由用户或 UI 层选择。
- **输出示例**：
  ```
  global memory directory:
    /home/user/.cc-rust/memory
  ```
- **边界情况**：
  - scope 为空：显示用法
  - 未知 scope：提示 "Unknown scope: 'xxx'. Use auto|team|global|project."
  - 目录不存在时自动创建（`fs::create_dir_all`）
  - 创建失败时：显示错误信息

### `/skills`

- **功能描述**：列出所有已加载的技能包。无参数或 `list` 按名称排序显示技能列表，含 source tag（bundled/user/project/plugin/mcp）、invocability tag（user/model/both）、版本和描述。支持 `--sort name|source|usage` 切换排序。传技能名则显示该技能的详细信息。
- **输出示例（列表）**：
  ```
  Available Skills (5 total)
  Registry revision: 42
  (sorted by name)
  ------------------------------------------------------------
    [bundled] (user) review@1.0.0 -- Review code changes
    [bundled] (both) update-config@1.0.0 -- Configure settings
    [user] (user) my-skill@0.1.0 -- Custom helper
  ...
  Use /skills <name> for details on a specific skill.
  Use /skills reload to hot-reload skill packages.
  Use /skills diagnostics to show validation diagnostics.
  Use /skills --sort <name|source|usage> to change sort order.
  ```
- **输出示例（无技能）**：
  ```
  No skills loaded.

  Bundled skills: simplify, remember, debug, stuck, update-config
  Place custom skills in ~/.cc-rust/skills/<name>/SKILL.md
  ```
- **输出示例（技能详情）**：
  ```
  Skill: Review
  Canonical name: review
  Source: Bundled
  Version: 1.0.0
  Description: Review code changes
  When to use: When the user asks to review a PR or diff
  Allowed tools: Bash, Read
  User invocable: true
  Model invocable: true
  Compatible app version: >=0.3.0
  ```
- **源码路径**：`crates/cc-commands/src/skills_cmd.rs`（`SkillsHandler`）
- **边界情况**：
  - 无技能时显示 "No skills loaded" 及默认技能路径说明
  - 未知技能名：显示 "Skill 'xxx' not found" 及提示
  - `reload` 需要 `PluginSkillsProvider` 注册；未注册时显示错误
  - `diagnostics`：显示所有加载诊断记录或 "No skill diagnostics recorded."
  - `--sort source`：按 bundled → user → project → plugin → mcp 排序
  - `--sort usage`：按滚动使用分数降序排序
  - score < 0.01 时不显示分数标签

### `/hooks`

- **功能描述**：显示合并后的 hook 树（按 event → matcher → hook 分组），聚合 managed/user/project/local 四层 settings 中的 hooks 声明。输出中每个 hook 带有 source badge 标注其来源层。
- **输出示例（完整树）**：
  ```
  Hooks
  ─────────────────────────────────────
  ├── PreToolUse
  │   └── matcher: Bash
  │       └── command: echo pre [user]
  └── Stop
      └── matcher: *
          └── command: notify-send "done" [project]

  Source precedence (low→high): managed → user → project → local.
  Use `/hooks open <layer>` to edit a specific settings file.
  ```
- **输出示例（无钩子）**：
  ```
  Hooks
  ─────────────────────────────────────
  (empty tree)

  Source precedence (low→high): managed → user → project → local.
  ```
- **源码路径**：`crates/cc-commands/src/hooks_cmd.rs`（`HooksHandler`）
- **边界情况**：
  - 无钩子：显示标题和 footer，tree 为空
  - settings 加载失败：在 footer 中显示 "Issues" 段
  - hook command 超过 80 字符：截断并加 "…" 后缀
  - 未知子命令：显示完整用法（含 layers 说明）
  - 已知事件顺序为：PreToolUse, PostToolUse, PostToolUseFailure, Stop, SubagentStart, SubagentStop, WorktreeCreate, WorktreeRemove, Notification；未知事件排在已知事件之后
  - hooks config 不是数组格式：报告为 issue，跳过该条目

### `/hooks list [event]`

- **功能描述**：列出 hook 树，可选按事件名过滤。无过滤时等价于 `/hooks`；带 event 参数时只显示该事件下的 matcher 和 hook。
- **输出示例（`/hooks list PreToolUse`）**：
  ```
  Hooks — PreToolUse
  ─────────────────────────────────────
  ├── PreToolUse
  │   └── matcher: Bash
  │       └── command: echo pre [user]
  ...
  ```
- **边界情况**：
  - 事件名大小写不敏感：`pretooluse` 和 `PreToolUse` 等价
  - 事件无匹配钩子：显示空树
  - 未知事件名：显示空树（不会报错）

### `/hooks path <scope>`

- **功能描述**：输出指定 settings 层的 settings.json 文件路径。
- **输出示例（`/hooks path user`）**：
  ```
  /home/user/.cc-rust/settings.json
  ```
- **边界情况**：
  - scope 为 `managed`/`policy`、`user`/`global`、`project`、`local`/`override` 均有效
  - 未知 scope：显示 "Usage: /hooks path|open <layer> where layer is one of: managed, user, project, local."

### `/hooks open <scope>`

- **功能描述**：创建（若不存在）并打开指定 settings 层的 settings.json 文件。缺失文件时用最小 JSON 骨架 `{ "hooks": {} }` 创建。
- **输出示例（成功）**：
  ```
  Opened /home/user/.cc-rust/settings.json
  ```
- **边界情况**：
  - 文件不存在：先创建骨架再打开
  - `$EDITOR`/`$VISUAL` 未设置：由 `ensure_and_open` 处理（可能回退）
  - 未知 scope：显示用法提示
