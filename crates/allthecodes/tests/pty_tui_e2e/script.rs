#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY 测试模板引擎 — 步骤式定义 TUI 测试用例。
//!
//! 通过 `TestStep` 枚举定义 TUI 操作序列，`TestRunner` 自动执行、截图、收集错误。
//! 每个测试有独立的输出文件夹，只保留 `.html` + `.log`。
//!
//! 用法：
//! ```rust
//! let case = TestCase::new("my_test")
//!     .step(TestStep::SkipTrustGate)
//!     .step(TestStep::Input("hello".into()))
//!     .step(TestStep::WaitForText("hello".into(), Duration::from_secs(30)))
//!     .step(TestStep::Snapshot("after_hello".into()));
//! TestRunner::new().run(&case).assert_no_errors();
//! ```

use crate::harness::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ─── TestStep ────────────────────────────────────────────────────────

/// 单个 TUI 操作步骤。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TestStep {
    /// 等待固定时长。
    Wait(Duration),
    /// 在 prompt 中输入文本并按 Enter。
    Input(String),
    /// 只输入文本，不按 Enter。
    TypeText(String),
    /// 执行斜杠命令（自动加 "/" 前缀 + Enter）。
    Command(String),
    /// 发送快捷键。
    Key(TestKey),
    /// 手动截图（保存 .html + .log）。
    Snapshot(String),
    /// 打开命令面板（发送 "/"），等待 "Commands" 出现。
    OpenPalette,
    /// 在命令面板中执行第 N 个条目（Down N-1 次 + Enter）。
    PaletteSelect(usize),
    /// 关闭命令面板（Esc）。
    ClosePalette,
    /// 断言屏幕包含指定文本。
    AssertScreenContains(String),
    /// 断言纯文本输出包含指定文本。
    AssertTextContains(String),
    /// 断言状态栏包含指定文本。
    AssertStatusBar(String),
    /// 断言屏幕不包含指定文本。
    AssertScreenNotContains(String),
    /// 等待指定文本出现在当前可见屏幕中（带超时）。
    WaitForScreenText(String, Duration),
    /// 断言当前 prompt/屏幕中包含指定文本。
    AssertPromptContains(String),
    /// 断言文件包含指定文本。
    AssertFileContains(String, String),
    /// 断言文件不包含指定文本。
    AssertFileNotContains(String, String),
    /// 等待文本出现在输出中（带超时）。
    WaitForText(String, Duration),
    /// 等待任意一个文本出现。
    WaitForAny(Vec<String>, Duration),
    /// 等待状态栏包含指定文本。
    WaitForStatus(String, Duration),
    /// 等待当前模型响应完成。
    WaitResponseDone(Duration),
    /// 多行输入（每行依次发送）。
    MultilineInput(Vec<String>),
    /// 跳过 workspace trust gate。
    SkipTrustGate,
    /// 通过 /permissions 设置权限（如 "full access"）。
    SetPermission(String),
    /// 通过 /login 切换 authProfile。
    LoginSwitch(String),
    /// 断言输出中无 "panicked"。
    AssertNoPanic,
    /// 批准权限对话框（发送 'y' 键，仅在屏幕上有权限文本时生效）。
    ApproveDialog,
}

/// 可发送的快捷键。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TestKey {
    CtrlC,
    CtrlD,
    CtrlU,
    CtrlL,
    CtrlR,
    F12,
    Enter,
    Escape,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Backspace,
    Delete,
    PageUp,
    PageDown,
    Home,
    End,
}

impl TestKey {
    fn send(&self, session: &PtySession) {
        match self {
            TestKey::CtrlC => session.send_ctrl_c(),
            TestKey::CtrlD => session.send_ctrl_d(),
            TestKey::CtrlU => session.send_ctrl_u(),
            TestKey::CtrlL => session.send_ctrl_l(),
            TestKey::CtrlR => session.send_ctrl_r(),
            TestKey::F12 => session.send_f12(),
            TestKey::Enter => session.send_raw(b"\r"),
            TestKey::Escape => session.send_escape(),
            TestKey::Tab => session.send_tab(),
            TestKey::Up => session.send_up(),
            TestKey::Down => session.send_down(),
            TestKey::Left => session.send_left(),
            TestKey::Right => session.send_right(),
            TestKey::Backspace => session.send_backspace(),
            TestKey::Delete => session.send_delete(),
            TestKey::PageUp => session.send_page_up(),
            TestKey::PageDown => session.send_page_down(),
            TestKey::Home => session.send_home(),
            TestKey::End => session.send_end(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            TestKey::CtrlC => "Ctrl+C",
            TestKey::CtrlD => "Ctrl+D",
            TestKey::CtrlU => "Ctrl+U",
            TestKey::CtrlL => "Ctrl+L",
            TestKey::CtrlR => "Ctrl+R",
            TestKey::F12 => "F12",
            TestKey::Enter => "Enter",
            TestKey::Escape => "Esc",
            TestKey::Tab => "Tab",
            TestKey::Up => "Up",
            TestKey::Down => "Down",
            TestKey::Left => "Left",
            TestKey::Right => "Right",
            TestKey::Backspace => "Backspace",
            TestKey::Delete => "Delete",
            TestKey::PageUp => "PageUp",
            TestKey::PageDown => "PageDown",
            TestKey::Home => "Home",
            TestKey::End => "End",
        }
    }
}

// ─── ExitMethod ──────────────────────────────────────────────────────

/// 测试结束时的退出方式。
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub enum ExitMethod {
    /// 双 Ctrl+C 确认退出，短等待。
    CtrlC,
    /// 双 Ctrl+D 确认退出，短等待。
    CtrlD,
    /// 立即结束（kill）。需要长等待时应放在具体 WaitFor* 步骤中。
    #[default]
    Kill,
}

// ─── TestCase ────────────────────────────────────────────────────────

/// 测试脚本定义。
#[derive(Clone, Debug)]
pub struct TestCase {
    /// 测试名称（用于文件夹和文件命名）。
    pub name: String,
    /// 终端列数。
    pub cols: u16,
    /// 终端行数。
    pub rows: u16,
    /// 附加环境变量。
    pub env: Vec<(String, String)>,
    /// 步骤序列。
    pub steps: Vec<TestStep>,
    /// 退出方式。
    pub exit: ExitMethod,
    /// 全局超时。
    pub timeout: Duration,
    /// 覆盖工作区目录（默认使用 `workspace()`）。
    pub workspace: Option<String>,
    /// 覆盖权限模式（默认 "bypass"）。
    pub permission_mode: Option<String>,
    /// 覆盖日志根目录（默认使用 `logs_dir()`）。
    pub log_root: Option<String>,
}

#[allow(dead_code)]
impl TestCase {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cols: 120,
            rows: 40,
            env: Vec::new(),
            steps: Vec::new(),
            exit: ExitMethod::default(),
            timeout: Duration::from_secs(120),
            workspace: None,
            permission_mode: None,
            log_root: None,
        }
    }

    pub fn step(mut self, step: TestStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn env(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.env.push((key.into(), val.into()));
        self
    }

    pub fn cols(mut self, cols: u16) -> Self {
        self.cols = cols;
        self
    }

    pub fn rows(mut self, rows: u16) -> Self {
        self.rows = rows;
        self
    }

    pub fn exit(mut self, exit: ExitMethod) -> Self {
        self.exit = exit;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn workspace(mut self, ws: impl Into<String>) -> Self {
        self.workspace = Some(ws.into());
        self
    }

    pub fn permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.permission_mode = Some(mode.into());
        self
    }

    pub fn log_root(mut self, root: impl Into<String>) -> Self {
        self.log_root = Some(root.into());
        self
    }
}

// ─── TestReport ──────────────────────────────────────────────────────

/// 单步执行结果。
#[derive(Debug)]
#[allow(dead_code)]
pub struct StepResult {
    pub index: usize,
    pub desc: String,
    pub snapshot: Option<PathBuf>,
    pub passed: bool,
    pub duration: Duration,
}

/// 单步错误。
#[derive(Debug, Clone)]
pub struct TestError {
    pub step_index: usize,
    pub desc: String,
    pub message: String,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "step {}: {} — {}", self.index(), self.desc, self.message)
    }
}

impl TestError {
    fn index(&self) -> usize {
        self.step_index + 1
    }
}

/// 测试执行报告。
#[allow(dead_code)]
pub struct TestReport {
    pub test_name: String,
    pub output_dir: PathBuf,
    pub steps: Vec<StepResult>,
    pub errors: Vec<TestError>,
    pub teardown_duration: Duration,
    pub session_log: PathBuf,
    pub session_html: PathBuf,
}

#[allow(dead_code)]
impl TestReport {
    /// 如果有任何错误，panic 并打印详情。
    pub fn assert_no_errors(&self) {
        if self.errors.is_empty() {
            eprintln!(
                "[report] {} — ALL {} steps passed",
                self.test_name,
                self.steps.len()
            );
        } else {
            let mut msg = format!(
                "[report] {} — {} errors:\n",
                self.test_name,
                self.errors.len()
            );
            for err in &self.errors {
                msg.push_str(&format!("  {}\n", err));
            }
            msg.push_str(&format!("Output dir: {}\n", self.output_dir.display()));
            panic!("{}", msg);
        }
    }

    /// 是否有错误。
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// 打印步骤汇总到 stderr。
    pub fn summary(&self) {
        eprintln!("\n=== {} ===", self.test_name);
        eprintln!("Output: {}", self.output_dir.display());
        eprintln!("{:<4} {:<6} {:<8} Description", "Step", "Time", "Status");
        eprintln!("{}", "-".repeat(70));
        for s in &self.steps {
            let status = if s.passed { "OK" } else { "FAIL" };
            let snap = s
                .snapshot
                .as_ref()
                .map(|p| format!(" → {}", p.file_name().unwrap_or_default().to_string_lossy()))
                .unwrap_or_default();
            eprintln!(
                "{:<4} {:<6} {:<8} {}{}",
                s.index + 1,
                format!("{:.0?}", s.duration),
                status,
                s.desc,
                snap
            );
        }
        if !self.errors.is_empty() {
            eprintln!("\nErrors:");
            for err in &self.errors {
                eprintln!("  {}", err);
            }
        }
        eprintln!();
    }

    /// 保存 errors.txt 到输出目录。
    pub fn save_errors(&self) {
        if self.errors.is_empty() {
            return;
        }
        let mut content = format!(
            "Test: {}\nErrors: {}\n\n",
            self.test_name,
            self.errors.len()
        );
        for err in &self.errors {
            content.push_str(&format!("{}\n", err));
        }
        let path = self.output_dir.join("errors.txt");
        std::fs::write(&path, content).expect("write errors.txt");
    }

    /// 保存 index.html，作为每个脚本化测试目录的人工排查入口。
    pub fn save_index(&self) {
        let status = if self.errors.is_empty() {
            "PASS"
        } else {
            "FAIL"
        };
        let mut html = String::new();
        html.push_str(&format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{test_name} - pty_tui_e2e</title>
<style>
body{{font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:24px;background:#f7f7f7;color:#202020}}
h1{{font-size:22px;margin:0 0 8px}}
.meta{{margin:0 0 16px;color:#555;line-height:1.5}}
table{{border-collapse:collapse;width:100%;background:#fff;border:1px solid #d8d8d8}}
th,td{{border-bottom:1px solid #e7e7e7;padding:8px 10px;text-align:left;vertical-align:top;font-size:14px}}
th{{background:#ececec;font-weight:600}}
tr.fail{{background:#fff1f1}}
code{{font-family:'Cascadia Code','Consolas','Courier New',monospace}}
a{{color:#005ea8;text-decoration:none}}
a:hover{{text-decoration:underline}}
.status-pass{{color:#137333;font-weight:700}}
.status-fail{{color:#b3261e;font-weight:700}}
</style></head><body>
<h1>{test_name}</h1>
<div class="meta">
status: <span class="status-{status_class}">{status}</span><br>
steps: <code>{steps}</code> | errors: <code>{errors}</code><br>
session: {session_links}{errors_link}
</div>
<table><thead><tr><th>Step</th><th>Status</th><th>Time</th><th>Description</th><th>Artifacts</th></tr></thead><tbody>
"#,
            test_name = html_escape(&self.test_name),
            status = status,
            status_class = if self.errors.is_empty() { "pass" } else { "fail" },
            steps = self.steps.len(),
            errors = self.errors.len(),
            session_links = artifact_links(&self.session_html),
            errors_link = if self.errors.is_empty() {
                String::new()
            } else {
                " | <a href=\"errors.txt\">errors.txt</a>".to_string()
            },
        ));

        for step in &self.steps {
            let status = if step.passed { "OK" } else { "FAIL" };
            let row_class = if step.passed { "" } else { " class=\"fail\"" };
            let artifacts = step
                .snapshot
                .as_ref()
                .map(|path| artifact_links(path))
                .unwrap_or_else(|| "&nbsp;".to_string());
            html.push_str(&format!(
                "<tr{row_class}><td><code>{index}</code></td><td>{status}</td><td><code>{duration:.0?}</code></td><td>{desc}</td><td>{artifacts}</td></tr>\n",
                index = step.index + 1,
                desc = html_escape(&step.desc),
                duration = step.duration,
            ));
        }

        html.push_str("</tbody></table></body></html>");
        let path = self.output_dir.join("index.html");
        std::fs::write(&path, html).expect("write index.html");
    }
}

// ─── TestRunner ──────────────────────────────────────────────────────

/// 步骤式测试执行引擎。
pub struct TestRunner {
    auto_snapshot: bool,
}

impl TestRunner {
    pub fn new() -> Self {
        Self {
            auto_snapshot: true,
        }
    }

    /// 禁用自动截图（每个非 Snapshot 步骤后不自动截图）。
    #[allow(dead_code)]
    pub fn no_auto_snapshot(mut self) -> Self {
        self.auto_snapshot = false;
        self
    }

    /// 执行测试用例，返回报告。
    pub fn run(&self, case: &TestCase) -> TestReport {
        let output_dir = match &case.log_root {
            Some(root) => {
                let dir = PathBuf::from(root).join(&case.name);
                std::fs::create_dir_all(&dir).expect("create output dir");
                dir
            }
            None => test_subdir(&case.name),
        };
        eprintln!("[runner] {} → {}", case.name, output_dir.display());

        // 启动 PTY 会话（始终使用真实 API key）
        let env_refs: Vec<(&str, &str)> = case
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let ws = case.workspace.as_deref().unwrap_or(workspace());
        let mode = case.permission_mode.as_deref().unwrap_or("bypass");
        let args: Vec<&str> = vec!["-C", ws, "--permission-mode", mode];
        let session = PtySession::spawn_with_env(
            &args, case.cols, case.rows,
            false, // strip_keys=false：始终使用真实 API key
            &env_refs,
        );

        let mut steps_result: Vec<StepResult> = Vec::new();
        let mut errors: Vec<TestError> = Vec::new();

        // 自动等待渲染 + 跳过 trust gate（如果步骤中有 SkipTrustGate，跳过自动处理）
        let has_trust_gate = case
            .steps
            .iter()
            .any(|s| matches!(s, TestStep::SkipTrustGate));
        if !has_trust_gate {
            std::thread::sleep(RENDER_WAIT);
            skip_trust_gate(&session);
        }

        let case_deadline = Instant::now() + case.timeout;
        for (i, step) in case.steps.iter().enumerate() {
            let start = Instant::now();
            let desc = step.describe();
            let mut snapshot_path: Option<PathBuf> = None;
            let mut passed = true;
            let mut case_timed_out = false;

            match self.execute_step(&session, step, &output_dir, i, case_deadline, case.timeout) {
                Ok(snap) => {
                    snapshot_path = snap;
                }
                Err(err) => {
                    passed = false;
                    case_timed_out = err.message.starts_with("case timeout of ");
                    errors.push(err);
                }
            }

            // 自动截图（非 Snapshot 步骤）
            if self.auto_snapshot
                && !case_timed_out
                && Instant::now() < case_deadline
                && !matches!(step, TestStep::Snapshot(_))
                && !matches!(step, TestStep::Wait(_))
            {
                let label = format!("step_{:03}_{}", i + 1, sanitize(&desc));
                let snap_text = session.snapshot_to(&label, &output_dir);
                if snapshot_path.is_none() {
                    snapshot_path = Some(output_dir.join(format!("{label}.html")));
                }
                // 对 Assert 步骤，用自动截图做二次检查
                if !passed {
                    let _ = snap_text; // 已在 execute_step 中记录错误
                }
            }

            if !case_timed_out && Instant::now() >= case_deadline {
                passed = false;
                case_timed_out = true;
                errors.push(TestError {
                    step_index: i,
                    desc: desc.clone(),
                    message: format!("case timeout of {:.0?} exceeded", case.timeout),
                });
            }

            steps_result.push(StepResult {
                index: i,
                desc,
                snapshot: snapshot_path,
                passed,
                duration: start.elapsed(),
            });

            if case_timed_out {
                break;
            }
        }

        // 退出。长时间等待应由具体 WaitFor* 步骤表达，teardown 只做短等待或立即终止。
        let session_label = "session_full";
        let teardown_start = Instant::now();
        let _output = match case.exit {
            ExitMethod::CtrlC => session.finish_after_quit_to(session_label, &output_dir),
            ExitMethod::CtrlD => session.finish_after_ctrl_d_to(session_label, &output_dir),
            ExitMethod::Kill => session.finish_to(Duration::ZERO, session_label, &output_dir),
        };
        let teardown_duration = teardown_start.elapsed();

        let session_log = output_dir.join(format!("{session_label}.log"));
        let session_html = output_dir.join(format!("{session_label}.html"));

        let report = TestReport {
            test_name: case.name.clone(),
            output_dir,
            steps: steps_result,
            errors,
            teardown_duration,
            session_log,
            session_html,
        };
        report.summary();
        report.save_errors();
        report.save_index();
        report
    }

    /// 执行单个步骤。
    fn execute_step(
        &self,
        session: &PtySession,
        step: &TestStep,
        output_dir: &std::path::Path,
        index: usize,
        case_deadline: Instant,
        case_timeout: Duration,
    ) -> Result<Option<PathBuf>, TestError> {
        let err = |msg: String| TestError {
            step_index: index,
            desc: step.describe(),
            message: msg,
        };
        let case_timeout_err = || err(format!("case timeout of {:.0?} exceeded", case_timeout));
        let bounded_timeout = |requested: Duration| {
            let remaining = case_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                Err(case_timeout_err())
            } else {
                Ok((requested.min(remaining), requested >= remaining))
            }
        };
        let sleep_bounded = |requested: Duration| {
            let (sleep_for, deadline_limited) = bounded_timeout(requested)?;
            std::thread::sleep(sleep_for);
            if deadline_limited {
                Err(case_timeout_err())
            } else {
                Ok(())
            }
        };

        if Instant::now() >= case_deadline {
            return Err(case_timeout_err());
        }

        match step {
            TestStep::Wait(d) => {
                sleep_bounded(*d)?;
                Ok(None)
            }

            TestStep::Input(text) => {
                session.send_line(text);
                Ok(None)
            }

            TestStep::TypeText(text) => {
                session.send_raw(text.as_bytes());
                Ok(None)
            }

            TestStep::Command(cmd) => {
                session.send_line(&format!("/{}", cmd));
                Ok(None)
            }

            TestStep::Key(key) => {
                key.send(session);
                Ok(None)
            }

            TestStep::Snapshot(label) => {
                let safe_label = format!("step_{:03}_{}", index + 1, sanitize(label));
                session.snapshot_to(&safe_label, output_dir);
                Ok(Some(output_dir.join(format!("{safe_label}.html"))))
            }

            TestStep::OpenPalette => {
                session.send_raw(b"/");
                // 等待命令面板出现
                let (timeout, deadline_limited) = bounded_timeout(Duration::from_secs(3))?;
                let found = session.wait_for_screen_text("Commands", timeout);
                if !found {
                    if deadline_limited {
                        return Err(case_timeout_err());
                    }
                    return Err(err("command palette did not appear".into()));
                }
                Ok(None)
            }

            TestStep::PaletteSelect(n) => {
                for _ in 0..*n {
                    session.send_down();
                    sleep_bounded(Duration::from_millis(100))?;
                }
                session.send_raw(b"\r");
                sleep_bounded(Duration::from_millis(300))?;
                Ok(None)
            }

            TestStep::ClosePalette => {
                session.send_escape();
                sleep_bounded(Duration::from_millis(300))?;
                Ok(None)
            }

            TestStep::AssertScreenContains(text) => {
                sleep_bounded(Duration::from_millis(300))?;
                let screen = session.current_screen();
                if screen.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!("screen does not contain '{}'", text)))
                }
            }

            TestStep::AssertTextContains(text) => {
                sleep_bounded(Duration::from_millis(300))?;
                let content = session.current_text();
                if content.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!("text does not contain '{}'", text)))
                }
            }

            TestStep::AssertStatusBar(text) => {
                sleep_bounded(Duration::from_millis(500))?;
                let bar = session.status_bar();
                if bar.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!(
                        "status bar '{}' does not contain '{}'",
                        bar, text
                    )))
                }
            }

            TestStep::AssertScreenNotContains(text) => {
                sleep_bounded(Duration::from_millis(300))?;
                let screen = session.current_screen();
                if !screen.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!("screen unexpectedly contains '{}'", text)))
                }
            }

            TestStep::WaitForScreenText(text, timeout) => {
                let (bounded, deadline_limited) = bounded_timeout(*timeout)?;
                let found = session.wait_for_screen_text(text, bounded);
                if found {
                    Ok(None)
                } else if deadline_limited {
                    Err(case_timeout_err())
                } else {
                    Err(err(format!(
                        "screen text '{}' not found within {:.0?}",
                        text, timeout
                    )))
                }
            }

            TestStep::AssertPromptContains(text) => {
                sleep_bounded(Duration::from_millis(300))?;
                let screen = session.current_screen();
                if screen.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!("prompt/screen does not contain '{}'", text)))
                }
            }

            TestStep::AssertFileContains(path, text) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| err(format!("failed to read '{}': {}", path, e)))?;
                if content.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!("file '{}' does not contain '{}'", path, text)))
                }
            }

            TestStep::AssertFileNotContains(path, text) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| err(format!("failed to read '{}': {}", path, e)))?;
                if !content.contains(text.as_str()) {
                    Ok(None)
                } else {
                    Err(err(format!(
                        "file '{}' unexpectedly contains '{}'",
                        path, text
                    )))
                }
            }

            TestStep::WaitForText(text, timeout) => {
                let (bounded, deadline_limited) = bounded_timeout(*timeout)?;
                let found = session.wait_for_text(text, bounded);
                if found {
                    Ok(None)
                } else if deadline_limited {
                    Err(case_timeout_err())
                } else {
                    Err(err(format!(
                        "text '{}' not found within {:.0?}",
                        text, timeout
                    )))
                }
            }

            TestStep::WaitForAny(texts, timeout) => {
                let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
                let (bounded, deadline_limited) = bounded_timeout(*timeout)?;
                let found = session.wait_for_any(&refs, bounded);
                if found.is_some() {
                    Ok(None)
                } else if deadline_limited {
                    Err(case_timeout_err())
                } else {
                    Err(err(format!(
                        "none of {:?} found within {:.0?}",
                        texts, timeout
                    )))
                }
            }

            TestStep::WaitForStatus(text, timeout) => {
                let (bounded, deadline_limited) = bounded_timeout(*timeout)?;
                let found = session.wait_status(text, bounded);
                if found {
                    Ok(None)
                } else if deadline_limited {
                    Err(case_timeout_err())
                } else {
                    Err(err(format!(
                        "status '{}' not found within {:.0?}",
                        text, timeout
                    )))
                }
            }

            TestStep::WaitResponseDone(timeout) => {
                let (bounded, deadline_limited) = bounded_timeout(*timeout)?;
                if session.wait_response_done(0, bounded) {
                    Ok(None)
                } else if deadline_limited {
                    Err(case_timeout_err())
                } else {
                    Err(err(format!("response not done within {:.0?}", timeout)))
                }
            }

            TestStep::MultilineInput(lines) => {
                for line in lines {
                    session.send_line(line);
                    sleep_bounded(Duration::from_millis(100))?;
                }
                Ok(None)
            }

            TestStep::SkipTrustGate => {
                sleep_bounded(RENDER_WAIT)?;
                sleep_bounded(Duration::from_millis(500))?;
                let screen = session.current_screen();
                if screen.contains("trust")
                    || screen.contains("Trust")
                    || screen.contains("safety check")
                {
                    session.send_raw(b"\r");
                    sleep_bounded(Duration::from_millis(500))?;
                }
                Ok(None)
            }

            TestStep::SetPermission(level) => {
                session.send_line(&format!("/permissions {}", level));
                sleep_bounded(Duration::from_secs(2))?;
                Ok(None)
            }

            TestStep::LoginSwitch(profile) => {
                session.send_line(&format!("/login {}", profile));
                sleep_bounded(Duration::from_secs(3))?;
                Ok(None)
            }

            TestStep::AssertNoPanic => {
                sleep_bounded(Duration::from_millis(200))?;
                let text = session.current_text();
                if text.contains("panicked") {
                    Err(err("output contains 'panicked'".into()))
                } else {
                    Ok(None)
                }
            }

            TestStep::ApproveDialog => {
                sleep_bounded(Duration::from_millis(500))?;
                let screen = session.current_screen();
                if screen.contains("Permission")
                    || screen.contains("permission")
                    || screen.contains("Allow")
                    || screen.contains("allow")
                {
                    session.send_raw(b"y");
                    sleep_bounded(Duration::from_secs(2))?;
                }
                Ok(None)
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

impl TestStep {
    fn describe(&self) -> String {
        match self {
            TestStep::Wait(d) => format!("wait {:.0?}", d),
            TestStep::Input(s) => format!("input '{}'", truncate(s, 40)),
            TestStep::TypeText(s) => format!("type '{}'", truncate(s, 40)),
            TestStep::Command(s) => format!("/{}", truncate(s, 40)),
            TestStep::Key(k) => k.name().to_string(),
            TestStep::Snapshot(s) => format!("snapshot '{}'", s),
            TestStep::OpenPalette => "open palette".into(),
            TestStep::PaletteSelect(n) => format!("palette select #{}", n),
            TestStep::ClosePalette => "close palette".into(),
            TestStep::AssertScreenContains(s) => format!("screen has '{}'", truncate(s, 30)),
            TestStep::AssertTextContains(s) => format!("text has '{}'", truncate(s, 30)),
            TestStep::AssertStatusBar(s) => format!("bar has '{}'", truncate(s, 30)),
            TestStep::AssertScreenNotContains(s) => format!("screen !has '{}'", truncate(s, 30)),
            TestStep::WaitForScreenText(s, d) => {
                format!("wait screen '{}' ({:.0?})", truncate(s, 20), d)
            }
            TestStep::AssertPromptContains(s) => format!("prompt has '{}'", truncate(s, 30)),
            TestStep::AssertFileContains(path, s) => {
                format!("file '{}' has '{}'", truncate(path, 24), truncate(s, 20))
            }
            TestStep::AssertFileNotContains(path, s) => {
                format!("file '{}' !has '{}'", truncate(path, 24), truncate(s, 20))
            }
            TestStep::WaitForText(s, d) => format!("wait '{}' ({:.0?})", truncate(s, 20), d),
            TestStep::WaitForAny(ss, d) => format!("wait any {:?} ({:.0?})", ss.len(), d),
            TestStep::WaitForStatus(s, d) => {
                format!("wait status '{}' ({:.0?})", truncate(s, 20), d)
            }
            TestStep::WaitResponseDone(d) => format!("wait response done ({:.0?})", d),
            TestStep::MultilineInput(ls) => format!("multiline ({} lines)", ls.len()),
            TestStep::SkipTrustGate => "skip trust".into(),
            TestStep::SetPermission(s) => format!("perm '{}'", s),
            TestStep::LoginSwitch(s) => format!("login '{}'", s),
            TestStep::AssertNoPanic => "no panic".into(),
            TestStep::ApproveDialog => "approve dialog".into(),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn artifact_links(path: &std::path::Path) -> String {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return String::new();
    };
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(file_name);
    let file_name = html_escape(file_name);
    let stem = html_escape(stem);
    format!(
        "<a href=\"{file_name}\">html</a> | <a href=\"{stem}.log\">log</a> | <a href=\"{stem}.stream.log\">stream</a> | <a href=\"{stem}.raw\">raw</a>"
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ─── 示例测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_flow::read_settings;

    const SCRIPTS_LOG_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/logs/pty_tui_e2e_scripts");

    /// 基础对话验证：读取 settings.json 中的 activeAuthProfile，验证模型回复。
    #[test]
    #[ignore = "requires real API key"]
    fn script_conversation_verify() {
        let settings = read_settings();
        let case = TestCase::new(format!("conv_verify_{}", settings.active_auth_profile))
            .log_root(SCRIPTS_LOG_ROOT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Wait(Duration::from_secs(2)))
            .step(TestStep::Snapshot("initial".into()))
            .step(TestStep::AssertStatusBar(settings.expected_model.clone()))
            .step(TestStep::Input("Answer with ONLY your model ID.".into()))
            .step(TestStep::WaitForAny(
                vec![settings.expected_model.clone(), "deepseek".into()],
                API_TIMEOUT,
            ))
            .step(TestStep::Snapshot("model_reply".into()))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::Key(TestKey::CtrlC));

        TestRunner::new().run(&case).assert_no_errors();
    }

    /// 切换 authProfile 测试：使用 /login 切换到 claude_code，验证 deepseek-v4-pro。
    #[test]
    #[ignore = "requires real API key"]
    fn script_switch_auth_profile() {
        let case = TestCase::new("switch_to_claude_code")
            .log_root(SCRIPTS_LOG_ROOT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Wait(Duration::from_secs(2)))
            .step(TestStep::Snapshot("before_switch".into()))
            .step(TestStep::LoginSwitch("claude-code".into()))
            .step(TestStep::Wait(Duration::from_secs(3)))
            .step(TestStep::Snapshot("after_login_switch".into()))
            .step(TestStep::AssertStatusBar("deepseek-v4-pro".into()))
            .step(TestStep::Input("Answer with ONLY your model ID.".into()))
            .step(TestStep::WaitForAny(vec!["deepseek".into()], API_TIMEOUT))
            .step(TestStep::Snapshot("new_model_reply".into()))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::Key(TestKey::CtrlC));

        TestRunner::new().run(&case).assert_no_errors();
    }

    /// 权限设置测试：设置 full access，验证工具执行。
    #[test]
    #[ignore = "requires real API key"]
    fn script_set_permissions() {
        let case = TestCase::new("permissions_full_access")
            .log_root(SCRIPTS_LOG_ROOT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Wait(Duration::from_secs(2)))
            .step(TestStep::SetPermission("full access".into()))
            .step(TestStep::Wait(Duration::from_secs(2)))
            .step(TestStep::Snapshot("after_permission".into()))
            .step(TestStep::Input(
                "Use Bash to run: echo PERMISSIONS_TEST_OK".into(),
            ))
            .step(TestStep::WaitForText(
                "PERMISSIONS_TEST_OK".into(),
                API_TIMEOUT,
            ))
            .step(TestStep::Snapshot("tool_executed".into()))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::Key(TestKey::CtrlC));

        TestRunner::new().run(&case).assert_no_errors();
    }

    /// Ctrl+C 中断 + 恢复测试。
    #[test]
    #[ignore = "requires real API key"]
    fn script_abort_and_recover() {
        let case = TestCase::new("abort_recover")
            .log_root(SCRIPTS_LOG_ROOT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Input(
                "Write a 2000-word essay about computing.".into(),
            ))
            .step(TestStep::Wait(Duration::from_secs(3)))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Snapshot("after_abort".into()))
            .step(TestStep::AssertNoPanic)
            .step(TestStep::Input("Say exactly: RECOVERED".into()))
            .step(TestStep::WaitForText("RECOVERED".into(), API_TIMEOUT))
            .step(TestStep::Snapshot("recovered".into()))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::Key(TestKey::CtrlC));

        TestRunner::new().run(&case).assert_no_errors();
    }

    /// /model 切换测试：验证切换后模型回复变化。
    #[test]
    #[ignore = "requires real API key"]
    fn script_model_switch() {
        let settings = read_settings();
        let new_model = if settings.expected_model.contains("5.4") {
            "gpt-5.4-mini"
        } else {
            "gpt-5.4"
        };
        let case = TestCase::new(format!("model_switch_{}", new_model.replace('.', "_")))
            .log_root(SCRIPTS_LOG_ROOT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Wait(Duration::from_secs(2)))
            .step(TestStep::Snapshot("before_switch".into()))
            .step(TestStep::Command(format!("model {new_model}")))
            .step(TestStep::Wait(Duration::from_secs(3)))
            .step(TestStep::Snapshot("after_model_cmd".into()))
            .step(TestStep::Input("Answer with ONLY your model ID.".into()))
            .step(TestStep::WaitForText(new_model.into(), API_TIMEOUT))
            .step(TestStep::Snapshot("new_model_confirmed".into()))
            .step(TestStep::Key(TestKey::CtrlC))
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::Key(TestKey::CtrlC));

        TestRunner::new().run(&case).assert_no_errors();
    }

    /// 命令面板打开 + 选择测试（离线，验证面板渲染）。
    #[test]
    fn script_command_palette() {
        let case = TestCase::new("command_palette_flow")
            .log_root(SCRIPTS_LOG_ROOT)
            .timeout(QUICK_TIMEOUT)
            .step(TestStep::SkipTrustGate)
            .step(TestStep::Wait(Duration::from_millis(500)))
            .step(TestStep::OpenPalette)
            .step(TestStep::Snapshot("palette_open".into()))
            .step(TestStep::AssertScreenContains("Commands".into()))
            .step(TestStep::ClosePalette)
            .step(TestStep::AssertNoPanic);

        TestRunner::new().run(&case).assert_no_errors();
    }

    #[test]
    fn script_default_kill_teardown_does_not_wait_case_timeout() {
        let case = TestCase::new("default_kill_fast_teardown")
            .log_root(SCRIPTS_LOG_ROOT)
            .timeout(Duration::from_secs(12))
            .step(TestStep::SkipTrustGate)
            .step(TestStep::AssertNoPanic);

        let report = TestRunner::new().no_auto_snapshot().run(&case);

        report.assert_no_errors();
        assert!(
            report.teardown_duration < Duration::from_secs(4),
            "default Kill teardown took {:.2?}; it should not wait for the case timeout",
            report.teardown_duration
        );
    }

    #[test]
    fn script_case_timeout_bounds_long_wait() {
        let case = TestCase::new("case_timeout_bounds_long_wait")
            .log_root(SCRIPTS_LOG_ROOT)
            .timeout(Duration::from_millis(150))
            .step(TestStep::Wait(Duration::from_secs(5)));

        let report = TestRunner::new().no_auto_snapshot().run(&case);

        assert!(
            report.has_errors(),
            "the case deadline should fail the long wait"
        );
        assert_eq!(report.steps.len(), 1);
        assert!(
            report.steps[0].duration < Duration::from_secs(1),
            "the 5s wait ran for {:.2?} instead of being bounded by the 150ms case deadline",
            report.steps[0].duration
        );
        assert!(
            report.errors[0].message.contains("case timeout"),
            "unexpected timeout diagnostic: {}",
            report.errors[0].message
        );
    }
}
