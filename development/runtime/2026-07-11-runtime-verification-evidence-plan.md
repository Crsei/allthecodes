# Runtime Verification Evidence and Session Report Implementation Plan

> **Implementation status (2026-07-13):** Tasks 1–6 and review remediation are implemented. Targeted, surface, UI, workspace fmt, and workspace release gates pass. The repository-wide clippy gate remains blocked by the pre-existing missing `allthecodes_mcp::take_installed_manager` test helper. See Task 6 for exact evidence.
>
> Commit-only checkboxes for this slice are completed in the implementation and documentation commits; future documentation updates remain separately scoped under `AGENTS.md`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn existing runtime records, tool results, audit events, OpenTelemetry spans, and cost events into an enforced verification loop and a durable, redacted Session Report that proves what an agent changed, ran, verified, spent, and left uncertain.

**Architecture:** Record/replay remains the canonical session history and `AuditContext` remains the correlated operational event stream. A verification gate inspects canonical tool evidence before a query or delegated task can claim success; missing evidence causes a bounded continuation nudge, so all commands still run through the normal tool and permission pipeline. A report projector reads canonical records and cost events to produce a versioned Session Report and JSONL/OTel-friendly summary without logging full prompts or secrets.

**Tech Stack:** Rust 1.91.1, serde/serde_json, existing `allthecodes-session` record/replay, `allthecodes-observability`, `allthecodes-engine`, `allthecodes-tools`, `allthecodes-services` OpenTelemetry/Langfuse, existing session cost log design.

## Global Constraints

- Do not add another transcript or event-log fact source; extend record/replay and observability.
- Do not execute verification commands through `tokio::process::Command` or a new shell path; missing evidence must drive normal model/tool continuation.
- Default event and report content is metadata-only or redacted; full prompts, full tool output, environment values, auth headers, and credentials are excluded.
- Store reports and artifacts under `ALLTHECODES_HOME`; project-local output is opt-in export only.
- Cost fields consume the event contract in `development/cost/session-cost-log-plan.md`; this plan must not invent a competing cost calculation.
- A report may state `unverified`; it must never turn missing evidence into a successful verification claim.
- Verification iteration defaults to three rounds and must terminate deterministically.
- Documentation updates are committed separately, as required by `AGENTS.md`.

## Existing Components to Reuse

- `crates/allthecodes-session/src/record_replay/types.rs`: versioned canonical `RecordItem`.
- `crates/allthecodes-session/src/audit_export.rs`: SHA-256 hash chain and tamper detection.
- `crates/allthecodes-observability/src/context.rs`: correlated `session_id`, `submit_id`, `turn_id`, request, tool, and parent event IDs.
- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`: turn transaction and final result boundary.
- `crates/allthecodes-engine/src/query/loop_impl.rs`: bounded continuation and token-budget control.
- `crates/allthecodes-tools/src/tool.rs`: `ToolResult::shell` structured exit evidence.
- `crates/allthecodes-permissions/src/command_risk.rs`: build/test/read/mutate/destructive/deploy/secret classification.
- `development/runtime/agent-runtime-execution-record-fields-plan.md`: execution-record field contract.
- `development/cost/session-cost-log-plan.md`: request/session cost facts and aggregation.

---

### Task 1: Add Versioned Verification and Artifact Record Items

**Files:**
- Modify: `crates/allthecodes-session/src/record_replay/types.rs`
- Modify: `crates/allthecodes-session/src/record_replay/reconstruct.rs`
- Modify: `crates/allthecodes-session/src/record_replay/tests.rs`

**Interfaces:**
- Consumes: existing `RecordLine` correlation and forward-compatible replay.
- Produces: canonical verification and artifact facts.

- [x] **Step 1: Write failing round-trip and unknown-field tests**

Add JSON round-trip tests for:

```rust
let item = RecordItem::VerificationFinished(VerificationFinishedRecord {
    verification_id: "verify-1".into(),
    policy: "targeted_tests".into(),
    round: 1,
    status: VerificationStatus::Passed,
    evidence_ids: vec!["evidence-1".into()],
    missing_requirements: vec![],
    unverified_assumptions: vec![],
});
let value = serde_json::to_value(&item).unwrap();
assert_eq!(value["type"], "verification_finished");
assert!(serde_json::from_value::<RecordItem>(value).is_ok());
```

- [x] **Step 2: Run record/replay tests and confirm failure**

```bash
cargo test -p allthecodes-session record_replay::tests::verification_record_round_trip -- --nocapture
```

Expected: compilation fails because the record variants do not exist.

- [x] **Step 3: Add the record contracts**

Add these variants to `RecordItem`:

```rust
VerificationStarted(VerificationStartedRecord),
VerificationEvidence(VerificationEvidenceRecord),
VerificationFinished(VerificationFinishedRecord),
ArtifactCreated(ArtifactCreatedRecord),
SessionReportGenerated(SessionReportGeneratedRecord),
```

Define the shared enums and records:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus { Passed, Failed, Incomplete }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind { Build, Test, Lint, SecurityGate, ApiSmoke, BrowserSmoke }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationEvidenceRecord {
    pub evidence_id: String,
    pub kind: EvidenceKind,
    pub tool_use_id: String,
    pub command_digest: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub artifact_ids: Vec<String>,
}
```

The remaining records carry IDs, policy name, round, status, missing requirements, redaction level, artifact digest, media type, byte length, and relative path. Never store raw secrets or full command output in these records.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationStartedRecord {
    pub verification_id: String,
    pub policy: String,
    pub round: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFinishedRecord {
    pub verification_id: String,
    pub policy: String,
    pub round: u8,
    pub status: VerificationStatus,
    pub evidence_ids: Vec<String>,
    pub missing_requirements: Vec<EvidenceKind>,
    pub unverified_assumptions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCreatedRecord {
    pub artifact_id: String,
    pub kind: String,
    pub media_type: String,
    pub digest: String,
    pub byte_len: u64,
    pub redaction: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReportGeneratedRecord {
    pub report_version: u32,
    pub relative_path: String,
    pub digest: String,
    pub record_head_digest: String,
}
```

- [x] **Step 4: Teach replay to preserve the new facts without changing message reconstruction**

The reconstructor records them in its event index and otherwise leaves message reconstruction unchanged. Older rollouts remain readable.

- [x] **Step 5: Run all session record/replay tests**

```bash
cargo test -p allthecodes-session record_replay -- --nocapture
```

Expected: PASS for old fixtures and new record types.

- [x] **Step 6: Commit the record contract**

```bash
git add -A -- crates/allthecodes-session/src/record_replay
git commit -m "Add canonical verification evidence records"
```

---

### Task 2: Classify Existing Tool Results as Verification Evidence

**Files:**
- Create: `crates/allthecodes-engine/src/verification/mod.rs`
- Create: `crates/allthecodes-engine/src/verification/evidence.rs`
- Create: `crates/allthecodes-engine/src/verification/policy.rs`
- Modify: `crates/allthecodes-engine/src/lib.rs`
- Test: `crates/allthecodes-engine/src/verification/evidence.rs`

**Interfaces:**
- Consumes: tool name, tool input, `ToolResult::shell`, command risk, and tool-use correlation ID.
- Produces: `Option<VerificationEvidenceRecord>` and a policy evaluator.

- [x] **Step 1: Write table-driven failing evidence tests**

Cover at least:

```rust
for (command, exit, expected) in [
    ("cargo build --workspace", 0, Some(EvidenceKind::Build)),
    ("cargo test -p allthecodes-tools", 0, Some(EvidenceKind::Test)),
    ("cargo fmt --all --check", 0, Some(EvidenceKind::Lint)),
    ("rm -rf /tmp/example", 0, None),
    ("cargo test", 101, None),
] {
    assert_eq!(classify_shell_evidence(command, exit), expected);
}
```

- [x] **Step 2: Run the focused test**

```bash
cargo test -p allthecodes-engine verification::evidence::tests -- --nocapture
```

Expected: FAIL because the verification module is absent.

- [x] **Step 3: Implement evidence classification**

Use `classify_command_risk` first. Only successful commands classified as `Build`, or explicit known test/lint/security commands, become positive evidence. Mutate, destructive, deploy, secret, unknown low-confidence, aborted, timed-out, and non-zero executions never count as passing evidence.

```rust
pub fn evidence_from_tool_result(
    tool_use_id: &str,
    tool_name: &str,
    input: &Value,
    result: &ToolResult,
) -> Option<VerificationEvidenceRecord>;
```

Hash the normalized command with SHA-256. Store the digest, not secret-bearing command text, in canonical records.

- [x] **Step 4: Implement named policies**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationPolicy {
    pub name: String,
    pub required: Vec<EvidenceKind>,
    pub max_rounds: u8,
}

pub fn policy_by_name(name: &str) -> anyhow::Result<VerificationPolicy> {
    match name {
        "none" => Ok(VerificationPolicy { name: name.into(), required: vec![], max_rounds: 0 }),
        "targeted_tests" => Ok(VerificationPolicy { name: name.into(), required: vec![EvidenceKind::Test], max_rounds: 3 }),
        "build_and_test" => Ok(VerificationPolicy { name: name.into(), required: vec![EvidenceKind::Build, EvidenceKind::Test], max_rounds: 3 }),
        "release_gate" => Ok(VerificationPolicy { name: name.into(), required: vec![EvidenceKind::Build, EvidenceKind::Test, EvidenceKind::Lint, EvidenceKind::SecurityGate], max_rounds: 3 }),
        other => anyhow::bail!("unknown verification policy: {other}"),
    }
}
```

- [x] **Step 5: Run focused tests**

```bash
cargo test -p allthecodes-engine verification -- --nocapture
git add -A -- crates/allthecodes-engine/src/verification crates/allthecodes-engine/src/lib.rs
git commit -m "Classify tool results as verification evidence"
```

---

### Task 3: Enforce a Bounded Verify-Continue Loop

**Files:**
- Modify: `crates/allthecodes-engine/src/types/config.rs`
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`
- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Modify: `crates/allthecodes-engine/src/query/turn_context.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`
- Test: `crates/allthecodes-engine/src/query/tests/control_tests.rs`

**Interfaces:**
- Consumes: `QueryEngineConfig::verification_policy` and evidence emitted during the turn.
- Produces: bounded continuation messages and an honest `Passed`, `Failed`, or `Incomplete` result.

- [x] **Step 1: Write failing loop-control tests**

Test these transitions:

```text
no policy                         -> normal completion
targeted_tests + no test evidence -> continuation round 1
test exit 0                       -> passed completion
test exit non-zero                -> continuation with failure summary
three missing rounds              -> incomplete completion, no fourth round
```

- [x] **Step 2: Add verification state to the turn context**

```rust
#[derive(Debug, Clone, Default)]
pub struct VerificationTracker {
    pub policy: Option<VerificationPolicy>,
    pub round: u8,
    pub evidence: Vec<VerificationEvidenceRecord>,
    pub failed_attempts: Vec<String>,
}
```

Add `verification_policy: Option<String>` to `QueryEngineConfig` using the repository's existing config-construction pattern. Update every explicit struct literal in the same commit; do not silence missing fields with broad `Default` conversions.

Forward `AgentInput::verification_policy` through `build_child_config` so delegated/background agents use the same named policy stored in their task envelope.

- [x] **Step 3: Record evidence at the canonical tool-result boundary**

After the tool pipeline returns a structured result, call `evidence_from_tool_result`, append `RecordItem::VerificationEvidence`, and update the current tracker. Use the same tool-use ID already present in the audit context.

- [x] **Step 4: Gate successful completion**

Before the query loop accepts a successful terminal model result:

```rust
match evaluate(&tracker) {
    VerificationDecision::Passed(summary) => finish_with_verification(summary),
    VerificationDecision::Continue { round, missing, failures } => {
        inject_verification_nudge(round, &missing, &failures);
        continue;
    }
    VerificationDecision::Incomplete(summary) => finish_unverified(summary),
}
```

The nudge names evidence categories and prior failing exit codes, not guessed commands. The model chooses tools through the normal permission path.

- [x] **Step 5: Run control and lifecycle tests**

```bash
cargo test -p allthecodes-engine query::tests::control_tests -- --nocapture
cargo test -p allthecodes-engine lifecycle::tests -- --nocapture
```

Expected: PASS; the loop always terminates at or before the configured round cap.

- [x] **Step 6: Commit the enforcement slice**

```bash
git add -A -- crates/allthecodes-engine/src/types/config.rs crates/allthecodes-engine/src/agent/mod.rs crates/allthecodes-engine/src/query crates/allthecodes-engine/src/lifecycle/deps/execute.rs
git commit -m "Enforce bounded runtime verification"
```

---

### Task 4: Project a Redacted Session Report

**Files:**
- Create: `crates/allthecodes-session/src/session_report.rs`
- Modify: `crates/allthecodes-session/src/lib.rs`
- Modify: `crates/allthecodes-session/src/record_replay/reader.rs`
- Test: `crates/allthecodes-session/src/session_report.rs`

**Interfaces:**
- Consumes: canonical rollout, audit metadata, verification evidence, worktree/session metadata, and optional session cost events.
- Produces: `SessionReportV1` and a report JSON file under the session run directory.

- [x] **Step 1: Write a failing golden projection test**

The expected DTO is:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReportV1 {
    pub schema_version: u32,
    pub generated_by: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub human_initiator: Option<String>,
    pub goal_id: Option<String>,
    pub agent_role: Option<String>,
    pub model: Option<String>,
    pub summary: String,
    pub changed_files: Vec<String>,
    pub commands: Vec<CommandEvidenceSummary>,
    pub verification: VerificationSummary,
    pub security_gate_passed: Option<bool>,
    pub risk_events: Vec<RiskEventSummary>,
    pub unverified_assumptions: Vec<String>,
    pub cost: Option<SessionCostReportSummary>,
    pub human_review_required: bool,
    pub reviewed_by_human: bool,
    pub record_head_digest: String,
}
```

Project, but do not recalculate, the `CostSummary` facts produced by `development/cost/session-cost-log-plan.md`. Define the report-only supporting summaries in the same module:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvidenceSummary {
    pub tool_use_id: String,
    pub risk: String,
    pub command_digest: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub policy: String,
    pub status: VerificationStatus,
    pub rounds: u8,
    pub evidence_ids: Vec<String>,
    pub missing_requirements: Vec<EvidenceKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEventSummary {
    pub rule_id: String,
    pub decision: String,
    pub payload_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCostReportSummary {
    pub total_tokens: u64,
    pub cache_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub api_calls: u64,
    pub unknown_pricing_count: u64,
    pub backfilled_count: u64,
}
```

- [x] **Step 2: Implement deterministic projection and redaction**

Sort changed files and evidence by stable keys. Command summaries contain risk class, digest, exit code, and duration, but not raw command text when it may contain credentials. Tool output is represented by digest and artifact ID.

- [x] **Step 3: Write the report atomically**

Write to:

```text
~/.allthecodes/runs/<session_id>/session-report.v1.json
```

Use a same-directory temporary file, `sync_all`, and atomic rename. Append `SessionReportGenerated` only after the rename succeeds.

- [x] **Step 4: Verify tamper linkage**

The report's `record_head_digest` must match the current canonical rollout/audit head. Changing a canonical record after report generation must make verification fail.

- [x] **Step 5: Run report and audit tests**

```bash
cargo test -p allthecodes-session session_report -- --nocapture
cargo test -p allthecodes-session audit_export -- --nocapture
git add -A -- crates/allthecodes-session/src/session_report.rs crates/allthecodes-session/src/lib.rs crates/allthecodes-session/src/record_replay/reader.rs
git commit -m "Generate redacted session evidence reports"
```

---

### Task 5: Expose Reports and Metrics Without Duplicating Facts

**Files:**
- Modify: `crates/allthecodes-ipc-protocol/src/`
- Modify: `crates/allthecodes-web/src/handlers/sessions.rs`
- Modify: `crates/allthecodes/src/ui/`
- Modify: `crates/allthecodes-services/src/telemetry/session_tracing.rs`
- Test: IPC, Web, UI, and telemetry tests.

**Interfaces:**
- Consumes: `SessionReportV1` and existing OTel exporters.
- Produces: read-only report APIs, UI summary, and metadata-only spans.

- [x] **Step 1: Add protocol and Web contract tests**

Add `GET /api/sessions/{session_id}/report` and the equivalent IPC response. Missing reports return a typed `not_generated` state, not HTTP 500.

- [x] **Step 2: Render a compact verification summary**

Show policy, status, evidence count, failed/missing requirements, risk-event count, total cost when available, and report integrity status. Do not render full tool output in the summary.

- [x] **Step 3: Emit metadata-only telemetry**

Emit span/event fields:

```text
session.id, verification.policy, verification.status,
verification.rounds, verification.evidence_count,
report.integrity_valid, cost.usd
```

Never emit prompt, tool output, proxy URL, environment, auth header, or local user path.

- [x] **Step 4: Run surface tests**

```bash
cargo test -p allthecodes-ipc-protocol -- --nocapture
cargo test -p allthecodes-web session_report -- --nocapture
cargo test -p allthecodes ui:: -- --nocapture
cargo test -p allthecodes-services telemetry -- --nocapture
git add -A -- crates/allthecodes-ipc-protocol crates/allthecodes-web/src/handlers/sessions.rs crates/allthecodes/src/ui crates/allthecodes-services/src/telemetry/session_tracing.rs
git commit -m "Expose runtime verification reports"
```

---

### Task 6: Integration Gates and Documentation Closure

**Files:**
- Create: `crates/allthecodes/tests/verification_report_e2e.rs`
- Modify: `docs/WORK_STATUS.md`
- Modify: `development/archive/IMPLEMENTATION_GAPS.md`
- Modify: `development/archive/plan/traceable-logging-plan.md`

- [x] **Step 1: Add deterministic acceptance tests**

Cover a passing test command, a failed test followed by a passing retry, missing evidence after three rounds, report redaction, report hash mismatch, cancellation, and an optional cost-event projection.

- [x] **Step 2: Run targeted gates**

```bash
cargo test -p allthecodes --test verification_report_e2e -- --nocapture
cargo test -p allthecodes-session session_report -- --nocapture
cargo test -p allthecodes-engine verification -- --nocapture
```

Expected: every test exits 0.

- [x] **Step 3: Update documentation**

Record which traceable-logging requirements are now implemented and retain any genuine residuals. Do not mark the entire logging plan complete unless every listed correlation and durable-event requirement is verified.

```bash
git add -A -- docs/WORK_STATUS.md development/archive/IMPLEMENTATION_GAPS.md development/archive/plan/traceable-logging-plan.md
git commit -m "Document runtime evidence reporting status"
```

- [x] **Step 4: Run final repository gates and record residual blockers**

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

Expected: every command exits 0 with no new warnings.

Actual 2026-07-13:

- `cargo build --workspace --release`: PASS.
- Task/surface gates: PASS — verification e2e 6, session report 3, engine verification 9, engine lifecycle 44, Web 1, IPC 116, telemetry 15, UI 811.
- `cargo fmt --all --check`: PASS; `git diff --check`: PASS.
- `cargo clippy -p allthecodes-session -p allthecodes-engine --all-targets -- -D warnings`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: BLOCKED outside this task by missing `allthecodes_mcp::take_installed_manager` in `crates/allthecodes-mcp/src/runtime.rs:124`.

## Completion Criteria

- Build, test, lint, security, API, and browser evidence use versioned canonical records.
- Required evidence is enforced through a bounded continuation loop using the normal tool/permission path.
- Missing evidence ends as `Incomplete`, never as a false pass.
- Session Report V1 is deterministic, redacted, tamper-linked, queryable, and viewable.
- OTel and cost summaries project existing facts rather than recalculating them independently.
