# Agentic Workflow Injection 与 Setup-chain 防护实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent untrusted issue, pull-request, comment, Web, MCP, memory, repository-instruction, and webhook content from silently driving shell execution, file mutation, workflow changes, downloads, deployment, or credential access.

**Architecture:** Add a conservative, turn-scoped taint ledger beside the existing session state instead of rewriting message serialization. Ingress adapters and untrusted tools attach provenance marks; the query engine propagates active marks into `ToolUseContext`; the canonical tool pipeline classifies the destination sink and applies deterministic allow/ask/deny rules before existing permission evaluation. Setup-chain and supply-chain scanners enrich the decision, while record/replay and audit events preserve redacted evidence.

**Tech Stack:** Rust 1.91.1, serde/serde_json, existing `allthecodes-permissions` command classifier and dangerous-command rules, `allthecodes-engine` tool pipeline, `allthecodes-tools`, MCP/Web/memory adapters, record/replay, audit export.

## Global Constraints

- Treat repository content and remote collaboration content as data, not authority; `AGENTS.md` remains an explicit instruction channel only through the existing instruction loader.
- Do not rely on system-prompt warnings as the enforcement boundary.
- Local user input is trusted as user intent but still passes destructive/deploy/secret permission rules.
- Repository-committed MCP configuration remains pending approval; this plan does not weaken existing MCP scope isolation.
- Taint may be cleared only by an explicit user approval scoped to the exact sink request or by deterministic sanitization that removes executable influence.
- A parser error, unknown sink, unknown domain, or ambiguous data flow fails closed to `Ask` or `Deny` according to risk.
- Never persist raw secrets, auth headers, environment values, or full remote payloads in security events.
- No scanner may execute repository code to inspect it.
- Documentation updates are committed separately.

## Existing Components to Reuse

- `crates/allthecodes-permissions/src/command_risk.rs`: unified risk classification.
- `crates/allthecodes-permissions/src/dangerous/shell.rs`: `curl|sh`, `wget|sh`, destructive, and secret-oriented rules.
- `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`: pre-execution security gate.
- `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`: `ToolUseContext` construction.
- `crates/allthecodes-tools/src/memory/mod.rs`: current untrusted LocalMemory wrapping and sanitization.
- `crates/allthecodes-engine/src/system_prompt/static_sections.rs`: advisory prompt-injection warning.
- `development/command/unified-command-risk-classification-plan.md`: command risk semantics.
- `development/kairos/mcp-tool-safety-analysis-plan.md`: MCP safety-profile integration.
- `crates/allthecodes-session/src/record_replay/types.rs` and audit export: durable, tamper-evident evidence.

---

### Task 1: Define Taint Sources, Marks, and Sink Decisions

**Files:**
- Create: `crates/allthecodes-types/src/security/mod.rs`
- Create: `crates/allthecodes-types/src/security/taint.rs`
- Modify: `crates/allthecodes-types/src/lib.rs`
- Modify: `crates/allthecodes-types/Cargo.toml`
- Test: `crates/allthecodes-types/src/security/taint.rs`

**Interfaces:**
- Consumes: source adapter metadata and tool-use IDs.
- Produces: stable serialized taint and decision contracts shared across engine, tools, IPC, and audit.

- [ ] **Step 1: Write failing serialization and merge tests**

```rust
let issue = TaintMark::from_content(UntrustedSourceKind::IssueBody, "issue:42", b"body");
let web = TaintMark::from_content(UntrustedSourceKind::WebContent, "tool:web-1", b"page");
let context = TaintContext::from_marks([issue.clone(), web.clone(), issue]);
assert_eq!(context.marks.len(), 2);
assert!(context.is_untrusted());
assert_eq!(serde_json::to_value(&context).unwrap()["marks"][0]["trust"], "untrusted");
```

- [ ] **Step 2: Run the focused test**

```bash
cargo test -p allthecodes-types security::taint::tests -- --nocapture
```

Expected: compilation fails because the security module does not exist.

- [ ] **Step 3: Add the source and trust contracts**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UntrustedSourceKind {
    IssueBody,
    PullRequestBody,
    ReviewComment,
    WebhookPayload,
    WebContent,
    McpResult,
    LocalMemory,
    RepositoryDocument,
    ToolOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel { TrustedUser, TrustedPolicy, Untrusted }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TaintMark {
    pub source: UntrustedSourceKind,
    pub source_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaintContext {
    pub marks: Vec<TaintMark>,
}
```

Provide the only content-to-mark constructor so callers cannot omit the digest:

```rust
impl TaintMark {
    pub fn from_content(
        source: UntrustedSourceKind,
        source_id: impl Into<String>,
        content: &[u8],
    ) -> Self {
        Self {
            source,
            source_id: source_id.into(),
            digest: sha256_hex(content),
        }
    }
}

fn sha256_hex(content: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content))
}

impl TaintContext {
    pub fn from_marks(marks: impl IntoIterator<Item = TaintMark>) -> Self {
        let mut marks: Vec<_> = marks.into_iter().collect();
        marks.sort_by(|a, b| {
            (&a.source, &a.source_id, &a.digest)
                .cmp(&(&b.source, &b.source_id, &b.digest))
        });
        marks.dedup();
        Self { marks }
    }

    pub fn is_untrusted(&self) -> bool {
        !self.marks.is_empty()
    }
}
```

Add `sha2 = { workspace = true }` to `crates/allthecodes-types/Cargo.toml`; use the workspace-pinned version.

`TaintContext::from_marks` sorts and deduplicates by `(source, source_id, digest)` for deterministic audit output.

- [ ] **Step 4: Add sink and policy decisions**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaintSink {
    ReadOnly,
    FileWrite,
    Shell,
    NetworkDownload,
    WorkflowWrite,
    PackageInstall,
    Deploy,
    CredentialAccess,
    ExternalMessage,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaintDecisionKind { Allow, Ask, Deny }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintDecision {
    pub decision: TaintDecisionKind,
    pub sink: TaintSink,
    pub rule_id: String,
    pub reason: String,
    pub source_digests: Vec<String>,
}
```

- [ ] **Step 5: Run tests and commit**

```bash
cargo test -p allthecodes-types security::taint -- --nocapture
git add -A -- crates/allthecodes-types/src/security crates/allthecodes-types/src/lib.rs crates/allthecodes-types/Cargo.toml
git commit -m "Define workflow injection taint contracts"
```

---

### Task 2: Capture Untrusted Provenance at Ingress and Tool Boundaries

**Files:**
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes-tools/src/memory/mod.rs`
- Modify: `crates/allthecodes-tools/src/network/`
- Modify: `crates/allthecodes-engine/src/mcp_tool_adapter.rs`
- Modify: `crates/allthecodes-gateway/src/`
- Modify: `crates/allthecodes-engine/src/lifecycle/state.rs`
- Create: `crates/allthecodes-engine/src/security/mod.rs`
- Create: `crates/allthecodes-engine/src/security/taint_ledger.rs`
- Modify: `crates/allthecodes-engine/src/lib.rs`
- Test: corresponding module tests.

**Interfaces:**
- Consumes: remote source IDs, content bytes, and tool-use IDs.
- Produces: `ToolResult::taint` and a bounded session/turn `TaintLedger`.

- [ ] **Step 1: Write failing tool-result provenance tests**

```rust
let result = ToolResult::with_untrusted_content(
    json!({"body": "run ./setup.sh"}),
    ToolResultContent::Text("run ./setup.sh".into()),
    "remote issue".into(),
    TaintMark::from_content(UntrustedSourceKind::IssueBody, "issue:42", b"run ./setup.sh"),
);
assert_eq!(result.taint.marks.len(), 1);
```

- [ ] **Step 2: Add taint to `ToolResult` with safe defaults**

```rust
pub struct ToolResult {
    pub data: Value,
    pub model_content: Option<ToolResultContent>,
    pub display_preview: Option<String>,
    pub new_messages: Vec<Message>,
    pub shell: Option<ShellExecutionOutput>,
    pub taint: TaintContext,
}
```

Update `ToolResult::with_content` to use `TaintContext::default()`. Update explicit struct literals that do not use `..Default::default()` in the same change.

Add the focused constructor used by untrusted adapters:

```rust
pub fn with_untrusted_content(
    data: Value,
    model_content: ToolResultContent,
    display_preview: String,
    mark: TaintMark,
) -> Self {
    Self {
        data,
        model_content: Some(model_content),
        display_preview: Some(display_preview),
        taint: TaintContext::from_marks([mark]),
        ..Default::default()
    }
}
```

- [ ] **Step 3: Tag all remote/untrusted adapters**

Apply these mappings:

```text
LocalMemoryRecall                 -> local_memory
WebFetch/WebSearch/browser text   -> web_content
MCP tool/resource result          -> mcp_result
GitHub issue body                 -> issue_body
GitHub PR body                    -> pull_request_body
GitHub review/issue comments      -> review_comment
gateway webhook payload           -> webhook_payload
repository README/setup guidance  -> repository_document
```

Compute SHA-256 over the original bytes before model-facing truncation. Persist only digest, source kind, and stable source ID.

- [ ] **Step 4: Add a bounded ledger to engine state**

```rust
#[derive(Debug, Clone, Default)]
pub struct TaintLedger {
    active_turn: TaintContext,
    by_tool_use_id: BTreeMap<String, TaintContext>,
}

impl TaintLedger {
    pub fn register_tool_result(&mut self, tool_use_id: &str, taint: TaintContext);
    pub fn active_context(&self) -> TaintContext;
    pub fn clear_at_trusted_turn_boundary(&mut self);
}
```

Limit stored marks per turn to 128. When the limit is exceeded, add one deterministic aggregate mark rather than dropping the fact that the turn is tainted.

- [ ] **Step 5: Verify adapters and commit**

```bash
cargo test -p allthecodes-tools memory -- --nocapture
cargo test -p allthecodes-tools network -- --nocapture
cargo test -p allthecodes-engine mcp_tool_adapter -- --nocapture
cargo test -p allthecodes-gateway -- --nocapture
git add -A -- crates/allthecodes-tools/src/tool.rs crates/allthecodes-tools/src/memory crates/allthecodes-tools/src/network crates/allthecodes-engine/src/mcp_tool_adapter.rs crates/allthecodes-engine/src/lifecycle/state.rs crates/allthecodes-engine/src/security crates/allthecodes-engine/src/lib.rs crates/allthecodes-gateway
git commit -m "Track untrusted runtime content provenance"
```

---

### Task 3: Classify Tool Sinks and Enforce Taint Before Permission Evaluation

**Files:**
- Create: `crates/allthecodes-permissions/src/taint_policy.rs`
- Modify: `crates/allthecodes-permissions/src/lib.rs`
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`
- Modify: `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`
- Modify: `crates/allthecodes-engine/src/tool_runtime/execution/tests.rs`

**Interfaces:**
- Consumes: active `TaintContext`, tool name/input, command risk, MCP safety profile.
- Produces: deterministic allow/ask/deny before ordinary permission evaluation.

- [ ] **Step 1: Write a failing policy matrix**

```rust
for (sink, expected) in [
    (TaintSink::ReadOnly, TaintDecisionKind::Allow),
    (TaintSink::FileWrite, TaintDecisionKind::Ask),
    (TaintSink::Shell, TaintDecisionKind::Ask),
    (TaintSink::WorkflowWrite, TaintDecisionKind::Deny),
    (TaintSink::Deploy, TaintDecisionKind::Deny),
    (TaintSink::CredentialAccess, TaintDecisionKind::Deny),
    (TaintSink::Unknown, TaintDecisionKind::Ask),
] {
    assert_eq!(decide_tainted_sink(&tainted(), sink).decision, expected);
}
```

- [ ] **Step 2: Implement sink classification**

```rust
pub fn classify_tool_sink(tool_name: &str, input: &Value) -> TaintSink;
pub fn decide_tainted_sink(
    taint: &TaintContext,
    sink: TaintSink,
    command_risk: Option<&CommandRisk>,
) -> TaintDecision;
```

Classify `Read/Grep/Glob/TaskList` as read-only; `Write/Edit/NotebookEdit` as file write, escalating `.github/workflows/**`, package scripts, hook configs, and executable scripts to workflow/package sinks; Bash/PowerShell by command risk; deployment and secret tools as their dedicated sinks; unknown tools as `Unknown`.

- [ ] **Step 3: Add `taint_context` to `ToolUseContext`**

```rust
pub struct ToolUseContext {
    // existing fields
    pub taint_context: TaintContext,
}
```

Populate it from `TaintLedger::active_context()` in `lifecycle/deps/execute.rs`. Update every test and adapter constructor explicitly with `TaintContext::default()`.

- [ ] **Step 4: Enforce the decision before ordinary tool permissions**

At the start of `security_validate`:

```rust
let sink = classify_tool_sink(tool_name, input);
let taint_decision = decide_tainted_sink(
    &ctx.taint_context,
    sink,
    command_risk_for(tool_name, input).as_ref(),
);
match taint_decision.decision {
    TaintDecisionKind::Allow => {}
    TaintDecisionKind::Ask => return require_taint_approval(taint_decision),
    TaintDecisionKind::Deny => return deny_tainted_sink(taint_decision),
}
```

`bypass` and auto permission modes do not bypass a deterministic taint `Deny`. An explicit user approval may satisfy only the exact `(source digests, tool name, sanitized input digest)` request.

- [ ] **Step 5: Run policy and pipeline tests**

```bash
cargo test -p allthecodes-permissions taint_policy -- --nocapture
cargo test -p allthecodes-engine tool_runtime::execution::tests -- --nocapture
```

Expected: PASS across default, auto, bypass, plan, and non-interactive modes.

- [ ] **Step 6: Commit the enforcement slice**

```bash
git add -A -- crates/allthecodes-permissions/src/taint_policy.rs crates/allthecodes-permissions/src/lib.rs crates/allthecodes-tools/src/tool.rs crates/allthecodes-engine/src/lifecycle/deps/execute.rs crates/allthecodes-engine/src/tool_runtime/execution
git commit -m "Enforce taint-aware tool permissions"
```

---

### Task 4: Add Setup-chain and Supply-chain Static Scanners

**Files:**
- Create: `crates/allthecodes-permissions/src/setup_chain.rs`
- Create: `crates/allthecodes-permissions/src/supply_chain.rs`
- Create: `crates/allthecodes-permissions/tests/fixtures/setup_chain/`
- Modify: `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`

**Interfaces:**
- Consumes: sanitized command, target file path/content, workspace root, and network host.
- Produces: deterministic findings that raise `Ask` or `Deny`.

- [ ] **Step 1: Add malicious and safe fixtures**

Fixtures must cover:

```text
README -> python setup -> DNS TXT -> decoded shell payload
package.json postinstall -> curl -> sh
Dockerfile RUN wget unknown binary
GitHub Action using owner/action@v4 floating tag
GitHub Action pinned to a 40-character SHA
normal cargo build/test without dynamic download
```

- [ ] **Step 2: Write failing scanner tests**

```rust
let findings = scan_setup_chain(workspace.path(), "python setup.py").unwrap();
assert!(findings.iter().any(|f| f.rule_id == "setup.dns_txt_payload"));
assert!(findings.iter().any(|f| f.decision == TaintDecisionKind::Deny));

let findings = scan_github_workflow(&pinned_fixture).unwrap();
assert!(!findings.iter().any(|f| f.rule_id == "supply.action_floating_ref"));
```

- [ ] **Step 3: Implement static rules without executing content**

Block by default:

```text
setup.dns_txt_payload
setup.curl_pipe_shell
setup.hidden_postinstall_download
setup.unknown_binary_download
setup.credential_file_access
```

Require approval:

```text
setup.dynamic_download
setup.network_shell_execution
setup.unknown_domain
setup.package_install_script
supply.action_floating_ref
supply.runner_unrestricted_egress
```

Approved registries/domains come from existing managed/user/project settings provenance. Repository content cannot add itself to the allowlist.

- [ ] **Step 4: Enforce file-write and shell scans**

Run scanners when:

- Bash/PowerShell invokes package install, setup, Docker build, DNS query, curl/wget, or a repository script.
- Write/Edit changes `package.json`, lockfiles, shell/Python setup scripts, Dockerfiles, hook configs, or `.github/workflows/**`.

Scanner parser failure produces `Ask` with a clear diagnostic; a matched block rule produces `Deny`.

- [ ] **Step 5: Run scanner tests and commit**

```bash
cargo test -p allthecodes-permissions setup_chain -- --nocapture
cargo test -p allthecodes-permissions supply_chain -- --nocapture
cargo test -p allthecodes-engine tool_runtime::execution -- --nocapture
git add -A -- crates/allthecodes-permissions/src/setup_chain.rs crates/allthecodes-permissions/src/supply_chain.rs crates/allthecodes-permissions/tests/fixtures/setup_chain crates/allthecodes-engine/src/tool_runtime/execution/security.rs
git commit -m "Block unsafe setup and supply chains"
```

---

### Task 5: Record Redacted Security Decisions and Surface Them to Users

**Files:**
- Modify: `crates/allthecodes-session/src/record_replay/types.rs`
- Modify: `crates/allthecodes-observability/src/event.rs`
- Modify: `crates/allthecodes-engine/src/tool_runtime/execution/pipeline.rs`
- Modify: `crates/allthecodes-ipc-protocol/src/`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/permissions.rs`
- Test: record, audit, IPC, and UI tests.

**Interfaces:**
- Consumes: `TaintDecision` and scanner findings.
- Produces: tamper-evident security events and actionable permission UI.

- [ ] **Step 1: Add failing record round-trip tests**

```rust
pub struct SecurityDecisionRecord {
    pub tool_use_id: String,
    pub tool_name: String,
    pub input_digest: String,
    pub sink: TaintSink,
    pub decision: TaintDecisionKind,
    pub rule_ids: Vec<String>,
    pub source_digests: Vec<String>,
    pub user_override: bool,
}
```

Add `RecordItem::SecurityDecision(SecurityDecisionRecord)` and the matching metadata-only observability event.

- [ ] **Step 2: Record every non-trivial decision**

Record `Ask`, `Deny`, explicit approval, scanner failure, and block findings. Raw remote payloads and raw secret-bearing commands are replaced by digests.

- [ ] **Step 3: Extend permission UI**

Display:

```text
Untrusted sources: PR description, MCP result
Target: Shell / package install
Decision: Approval required
Rules: setup.dynamic_download, awi.untrusted_to_shell
```

The approval action states that it applies only to this exact request. `/permissions` recent denials includes the same reason and rule IDs.

- [ ] **Step 4: Verify audit and UI behavior**

```bash
cargo test -p allthecodes-session record_replay -- --nocapture
cargo test -p allthecodes-observability -- --nocapture
cargo test -p allthecodes-ipc-protocol -- --nocapture
cargo test -p allthecodes ui::command_surface::surfaces::permissions -- --nocapture
```

Expected: PASS; snapshots contain no full remote payload or secret.

- [ ] **Step 5: Commit the audit/UI slice**

```bash
git add -A -- crates/allthecodes-session/src/record_replay/types.rs crates/allthecodes-observability/src/event.rs crates/allthecodes-engine/src/tool_runtime/execution/pipeline.rs crates/allthecodes-ipc-protocol crates/allthecodes/src/ui/command_surface/surfaces/permissions.rs
git commit -m "Audit workflow injection decisions"
```

---

### Task 6: End-to-End Attack Fixtures and Release Gate

**Files:**
- Create: `crates/allthecodes/tests/agentic_workflow_injection_e2e.rs`
- Create: `crates/allthecodes/tests/fixtures/awi/`
- Modify: `docs/WORK_STATUS.md`
- Modify: `development/archive/IMPLEMENTATION_GAPS.md`
- Modify: `development/archive/KNOWN_ISSUES.md` only if a user-visible limitation remains.

- [ ] **Step 1: Build deterministic attack scenarios**

Cover:

```text
PR body -> model tool call -> curl|sh                         => deny
issue comment -> Write .github/workflows/release.yml          => deny
MCP result -> npm install package with postinstall download   => deny
Web content -> ordinary source-code Edit                      => ask
Local user prompt -> cargo test                               => existing permission behavior
tainted read-only Grep                                        => allow
explicit approval -> exact sanitized request                  => allow once
explicit approval -> modified command                         => ask again
```

- [ ] **Step 2: Assert durable evidence**

For each blocked path, assert a `SecurityDecisionRecord` exists, audit hash verification passes, the dangerous tool body was not called, and no credential path or raw remote payload appears in exported JSON.

- [ ] **Step 3: Run the focused security gate**

```bash
cargo test -p allthecodes --test agentic_workflow_injection_e2e -- --nocapture
```

Expected: every scenario passes without credentials or network.

- [ ] **Step 4: Update documentation in its own commit**

Document the conservative turn-scoped propagation model, exact approval scope, supported ingress adapters, static scanner coverage, and any remaining false-positive/false-negative boundary.

```bash
git add -A -- docs/WORK_STATUS.md development/archive/IMPLEMENTATION_GAPS.md development/archive/KNOWN_ISSUES.md
git commit -m "Document workflow injection defenses"
```

- [ ] **Step 5: Run final repository gates**

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p allthecodes --test agentic_workflow_injection_e2e -- --nocapture
cargo build --workspace --release
```

Expected: every command exits 0 with no new warnings.

## Completion Criteria

- Every supported remote or repository-derived input has a stable untrusted provenance mark.
- Active taint reaches the canonical tool security pipeline.
- Read-only operations remain usable; risky writes and shell operations require exact approval; workflow/deploy/credential flows are denied by default.
- Setup-chain and supply-chain scanners detect the listed dynamic-download, postinstall, DNS payload, unknown binary, and floating-action-ref patterns.
- Decisions are visible, redacted, durable, and covered by the existing tamper-evident audit chain.
- Bypass/auto modes cannot silently override deterministic taint denial.
