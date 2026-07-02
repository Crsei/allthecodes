# 统一命令风险分类修改计划

## 背景

当前命令安全相关逻辑分散在四条链路：

- `allthecodes-permissions::dangerous`：Bash / PowerShell 危险命令硬规则。
- `allthecodes-permissions::read_only_shell`：Plan / Explore 模式的只读命令白名单。
- `allthecodes-tool-display::shell_heuristic`：UI 展示用的操作类型和风险等级。
- `allthecodes-safety::classifier`：Auto 模式和 Auto Review 的模型安全分类器。

这些链路对同一命令会产生不同语义。例如 `rm -rf target` 在 UI 展示中是
`Destructive`，但 Bash 硬规则主要拦截根目录、家目录、设备、数据库等高破坏面；
`terraform destroy` / `pulumi destroy` 当前没有统一规则覆盖。目标是新增一个共享的
命令风险分类层，作为权限、UI、Auto classifier、审计事件的共同输入。

## 目标

新增统一风险等级：

```rust
pub enum CommandRiskLevel {
    Read,
    Build,
    Mutate,
    Destructive,
    Deploy,
    Secret,
}
```

提供统一分类函数：

```rust
pub fn classify_command_risk(command: &str, shell: ShellKind) -> CommandRisk;
```

其中 `CommandRisk` 至少包含：

```rust
pub struct CommandRisk {
    pub level: CommandRiskLevel,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub segments: Vec<CommandSegmentRisk>,
    pub confidence: CommandRiskConfidence,
}
```

验收目标：

- 所有 Bash / PowerShell 命令在执行前都能得到统一风险结果。
- UI 展示、权限提示、Auto Review 风险字段、审计日志使用同一分类结果。
- 现有危险命令硬拦截不降级；新分类只扩大可见性和一致性。
- 覆盖用户期望的基础规则：`read`、`build`、`mutate`、`destructive`、`deploy`、
  `secret`。

## 非目标

- 不在第一阶段重写 sandbox、权限规则匹配或 Auto 模式模型提示。
- 不把所有 `mutate` 命令直接拒绝；分类结果先作为决策输入，最终行为仍由权限模式、
  deny/ask/allow 规则、sandbox 和用户确认决定。
- 不依赖简单 substring 作为唯一安全判断；能复用现有 shell parser 的地方继续复用。

## 分类语义

优先级从高到低：

1. `Secret`：可能读取、打印、上传、复制或导出凭据、token、password、keychain、
   credential store、`.env`、SSH private key、cloud credentials 等。
2. `Destructive`：不可逆删除、历史覆盖、磁盘/数据库/基础设施销毁。
3. `Deploy`：发布、推送、上线、远端状态变更、生产资源变更。
4. `Build`：构建、测试、格式化、lint、代码生成等验证或本地产物生成。
5. `Read`：只读查询，且必须通过 shell 语法安全校验。
6. `Mutate`：默认兜底，表示会或可能修改本地状态，但未命中更高风险。

复合命令按最高风险段决定整体风险。解析失败、动态 shell 语法、重定向、管道、命令替换
不能证明只读时，不得归类为 `Read`，至少归为 `Mutate`；如果命中危险/secret 模式，
归为对应更高风险。

## 初始规则集

### Read

只在通过现有 `read_only_shell` fail-closed 校验时返回：

- `cat`, `ls`, `grep`, `rg`
- `git status`, `git diff`, `git log`, `git show`
- `gh` / `docker` / `pyright` 中现有只读 map 覆盖的命令

带重定向、管道、变量展开、命令替换、heredoc、UNC credential leak 风险时不能是 `Read`。

### Build

初始覆盖：

- `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt`
- `npm run build`, `npm test`, `npm run test`, `pnpm/yarn/bun build|test`
- `pytest`, `vitest`, `jest`, `mocha`
- `make`, `cmake`, `go build`, `go test`, `rustc`

`Build` 命令如果包含发布、上传、删除、secret 暴露或危险 shell 片段，按更高优先级分类。

### Mutate

默认兜底；典型覆盖：

- `mkdir`, `touch`, `cp`, `mv`
- `sed -i`, `perl -pi`, `python`/`node` 脚本执行
- 未知命令、动态 shell 命令、写文件重定向

### Destructive

初始覆盖：

- `rm -rf`, `rm -r -f`, `rm --recursive --force`
- `git reset --hard`
- `git clean -fd`, `git clean -dfx`, `git clean --force`，但保留 `--dry-run` 为非破坏
- `git push --force`, `git push -f`, `git push --force-with-lease`
- `git stash drop`, `git stash clear`
- `terraform destroy`, `pulumi destroy`
- `DROP TABLE`, `DROP DATABASE`, `TRUNCATE TABLE`, `TRUNCATE DATABASE`
- `dd if=`, `mkfs`, 写入 `/dev/sd*` / `/dev/nvme*`
- PowerShell `Remove-Item -Recurse -Force`, `Clear-Disk`, `Format-Volume`,
  `Stop-Computer`, `Restart-Computer`

### Deploy

初始覆盖：

- `git push` 非 force
- `npm publish`, `cargo publish`
- `docker push`
- `kubectl apply`, `kubectl delete`, `kubectl rollout`, `helm upgrade`, `helm install`
- `terraform apply`, `pulumi up`
- 常见 cloud CLI 变更：`aws * create|update|delete|put`, `gcloud * deploy|update|delete`,
  `az * create|update|delete`

如果 deploy 命令同时是 destructive，例如 `kubectl delete` 或 cloud delete，可按
`Destructive` 或 `Deploy` 中更高优先级策略处理。建议第一版把显式 delete 类远端变更归
`Destructive`，非删除上线归 `Deploy`。

### Secret

初始覆盖：

- 读取或打印 credential 文件：`cat ~/.ssh/id_*`, `cat ~/.aws/credentials`,
  `cat ~/.config/gcloud/*`, `cat .env`, `cat .npmrc`
- 打印或导出 secret env：`echo $*_TOKEN`, `echo $*_KEY`, `printenv`, `env` 中命中
  `token|secret|password|passwd|api_key|apikey|access_token|refresh_token`
- 复制 secret 到剪贴板或网络：`pbcopy`, `xclip`, `curl`, `wget`, `scp` 等组合命中 secret
  来源。
- PowerShell credential 相关：`Get-Secret`, `Get-Credential`, `Export-Clixml` 结合
  credential 变量或 secret path。

分类器 prompt 中继续 redaction；统一风险分类不得记录 secret 原文到日志。

## 实现落点

### 1. 新增共享模块

建议落在 `crates/allthecodes-permissions/src/command_risk.rs`，并从
`crates/allthecodes-permissions/src/lib.rs` 导出。

理由：

- 现有危险命令和只读命令均在 `allthecodes-permissions`。
- engine、safety、tool-display 都已依赖或可合理依赖 permissions 层。
- 先不新增 crate，避免扩大 workspace 依赖和发布面。

模块结构：

```text
crates/allthecodes-permissions/src/command_risk.rs
crates/allthecodes-permissions/src/command_risk/bash.rs
crates/allthecodes-permissions/src/command_risk/powershell.rs
crates/allthecodes-permissions/src/command_risk/patterns.rs
```

如第一版希望控制改动面，也可以先做单文件 `command_risk.rs`，待规则增长后再拆分。

### 2. 复用现有能力

- `Read` 判断复用 `read_only_shell::{is_read_only_bash_command,
  is_read_only_powershell_command}`。
- `Destructive` 初始规则迁移/复用 `dangerous::{is_dangerous_command,
  is_dangerous_powershell_command}`，再补齐泛化 `rm -rf`、Terraform、Pulumi。
- shell 分段复用 `allthecodes_utils::bash::split_compound_command` 或
  `allthecodes-shell-command` parser；复合命令按段分类后取最高风险。
- UI 分类可继续保留 kind/subtype/target，但 risk level 来源改为统一 `CommandRisk` 映射。

### 3. 接入执行安全链路

修改 `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`：

- 在 Bash / PowerShell 危险命令检查处调用 `classify_command_risk`。
- 若 `level == Destructive` 且命中硬拦截规则，继续返回 `Dangerous command blocked`。
- 对 `Secret` 先采用 `Ask` 或阻断策略需要产品确认；建议第一版在非 Bypass 模式下阻断
  明确 secret exfiltration，普通 secret read 走 `Ask`。
- 审计事件附加 `command_risk_level`、`command_risk_reason`、`matched_rule`。

### 4. 接入权限提示和 Auto Review

修改 `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`：

- `permission_risk_level()` 改为调用统一分类。
- `permission_action_summary()` 可以保留现有摘要，但 Auto Review event 附带统一风险。
- Auto Review prompt 的 classifier input 增加结构化字段：

```json
{
  "command_risk": {
    "level": "destructive",
    "reason": "...",
    "matched_rule": "git reset --hard"
  }
}
```

### 5. 接入 Auto 模式 classifier

修改 `crates/allthecodes-safety/src/classifier.rs`：

- `SafetyClassifierRequest::is_high_risk()` 改为基于 `CommandRisk`。
- `Destructive` / `Secret` / `Deploy` 触发 thinking stage。
- classifier prompt 中增加统一风险上下文，但保持 secret redaction。

### 6. 接入 UI 展示

修改 `crates/allthecodes-tool-display/src/shell_heuristic.rs`：

- 继续负责 `OperationKind`、`OperationSubtype`、`target`。
- `OperationRisk` 从 `CommandRiskLevel` 映射：
  - `Read` -> `Safe`
  - `Build` -> `Medium`
  - `Mutate` -> `Medium`
  - `Deploy` -> `High`
  - `Secret` -> `High`
  - `Destructive` -> `Destructive`

后续可考虑把 UI 的 `OperationRisk` 直接替换为共享风险等级，但第一版避免破坏 UI API。

## 分阶段执行

### Phase 1：纯分类 API

- 新增 `CommandRiskLevel`、`CommandRisk`、`classify_command_risk`。
- 覆盖 Bash 基础规则和现有 dangerous/read-only 复用。
- 补单元测试，不接入执行链路。

验收：

- `git reset --hard` -> `Destructive`
- `git clean -fd` -> `Destructive`
- `git clean -fdn` -> 非 `Destructive`
- `rm -rf target` -> `Destructive`
- `terraform destroy` / `pulumi destroy` -> `Destructive`
- `npm run build` / `cargo build` / `pytest` / `npm test` -> `Build`
- `cat`, `ls`, `rg`, `git status`, `git diff` 简单命令 -> `Read`
- 未知命令 -> `Mutate`

### Phase 2：执行链路只读接入

- `security_validate` 读取统一风险。
- 保持现有硬拦截语义，先只替换风险来源和审计字段。
- `permission_risk_level()` 改为统一映射。

验收：

- 现有危险命令测试不回退。
- Auto Review event 风险字段来自 `CommandRisk`。
- 审计 JSON 包含统一风险字段。

### Phase 3：UI 和 Auto classifier 接入

- `shell_heuristic` 使用统一风险映射。
- `SafetyClassifierRequest::is_high_risk()` 使用统一风险。
- Auto classifier prompt 增加结构化风险上下文。

验收：

- UI 对 `rm -rf target`、`terraform destroy`、`npm publish` 展示一致风险。
- Auto 模式对 `Destructive` / `Secret` / `Deploy` 进入 thinking stage。
- redaction 测试仍保证 secret 不进入 prompt 原文。

### Phase 4：策略收紧和配置化

- 增加配置项控制 `Secret`、`Deploy`、`Destructive` 的默认行为：

```json
{
  "permissions": {
    "commandRisk": {
      "destructive": "deny",
      "secret": "ask",
      "deploy": "ask"
    }
  }
}
```

- 支持 managed policy 覆盖。
- 文档更新到命令/权限说明。

## 测试计划

### 单元测试

- `allthecodes-permissions::command_risk`：
  - 每个风险等级至少 8 个 Bash case。
  - PowerShell destructive / secret case。
  - 复合命令最高风险优先。
  - parser failure fail-closed。

- `read_only_shell` 回归：
  - 简单 read 命令仍 read。
  - 带管道、重定向、变量展开不能 read。

- `dangerous` 回归：
  - 现有 dangerous 测试全部保留。
  - 新增 terraform/pulumi/rm-rf-general。

### 集成测试

- `security_validate`：
  - Bypass 仍跳过安全验证。
  - Plan 模式仍只允许 read。
  - Destructive 命令执行前被阻断或提示。

- lifecycle permission：
  - Auto Review risk_level 来自统一分类。
  - `always_allow` 不绕过 managed deny。

- UI snapshot：
  - Bash command render 中 risk label 与统一分类一致。

### e2e

新增一个命令风险 e2e 分组：

```text
development/archive/plan/command-e2e-test-plan-11-command-risk.md
```

覆盖：

- read command no prompt in Plan / Explore。
- build command 在 Default/Auto 中按权限模式处理。
- destructive command 在非 Bypass 下不可静默执行。
- deploy / secret 命令必须有明确风险提示。

## 文档更新

- `development/archive/IMPLEMENTATION_GAPS.md`：移除或更新“危险命令策略分散”的 TODO。
- `docs/WORK_STATUS.md`：完成后补活跃待办状态。
- 命令参考文档：新增风险等级说明和用户可见行为。
- 如保留任何缩减，例如 PowerShell secret case 不完整，必须标注为 Intentional。

## 风险与缓解

- 误杀构建脚本：先把统一分类接入提示和审计，再逐步收紧 deny。
- 正则误报 secret：分类结果不记录 secret 原文，只记录规则名和原因。
- UI 行为变化过大：第一版只映射现有 `OperationRisk`，不直接替换 UI enum。
- 权限模式冲突：保持 `Bypass` 现有语义，但在 UI 和审计中仍展示高风险。
- 复合命令解析不足：解析失败按 fail-closed，不归类为 `Read`。

## 完成标准

- 统一分类 API 已导出并有单元测试。
- Bash / PowerShell 执行前均可拿到 `CommandRisk`。
- `read/build/mutate/destructive/deploy/secret` 六类都有测试覆盖。
- UI、Auto Review、Auto classifier 至少有一条链路消费统一风险。
- `cargo test -p allthecodes-permissions`、相关 engine/tool-display 测试通过。
- `cargo build --workspace --release` 无新增 warning。
