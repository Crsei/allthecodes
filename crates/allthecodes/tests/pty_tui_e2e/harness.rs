#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY 测试 harness — 封装伪终端的创建、输入输出和屏幕断言。
//!
//! 提供 `PtySession`：在真实 PTY 中启动 `allthecodes`，模拟键盘输入，
//! 通过 `vt100` 解析器读取当前屏幕内容，用于 TUI 行为断言。

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

// ─── 路径与配置 ──────────────────────────────────────────────────────

fn resolve_workspace_path(explicit: Option<&str>, runner_pid: u32) -> String {
    explicit
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("/tmp/cc-rust-e2e-test-{runner_pid}"))
}

/// 测试工作区目录。优先使用 `E2E_WORKSPACE` 环境变量；默认按 runner 进程
/// PID 隔离。nextest 每个 test case 使用独立进程，因此并发 test case 不再共享
/// `settings.json` / `sessions.db`。libtest 同一进程内仍须使用 `--test-threads=1`。
pub fn workspace() -> &'static str {
    static WS: OnceLock<String> = OnceLock::new();
    WS.get_or_init(|| {
        let explicit = std::env::var("E2E_WORKSPACE").ok();
        let dir = resolve_workspace_path(explicit.as_deref(), std::process::id());
        std::fs::create_dir_all(&dir).ok();
        dir
    })
}

/// 日志输出目录。
///
/// Phase 2 起并发：进程级 timestamp 目录 + PID 后缀，保证 nextest 多进程
/// 同秒跑也写不同目录（PID 进程级别唯一）。`test_subdir(test_name)` 再按名隔离。
pub fn logs_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let now = chrono::Local::now();
        let pid = std::process::id();
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("logs")
            .join(format!("pty_tui_e2e_{}_{}", now.format("%Y%m%d%H%M"), pid));
        std::fs::create_dir_all(&dir).expect("create logs dir");
        dir
    })
}

/// 创建并返回测试专属子目录：`logs/pty_tui_e2e_{timestamp}/{test_name}/`。
pub fn test_subdir(test_name: &str) -> PathBuf {
    let dir = logs_dir().join(test_name);
    std::fs::create_dir_all(&dir).expect("create test subdir");
    dir
}

/// 解析二进制路径：cargo test 环境用 cargo_bin()，否则查 PATH。
pub fn binary_path() -> PathBuf {
    match std::panic::catch_unwind(|| assert_cmd::cargo::cargo_bin("allthecodes")) {
        Ok(p) if p.exists() => p,
        _ => which::which("allthecodes")
            .unwrap_or_else(|_| panic!("allthecodes binary not found via cargo_bin or PATH")),
    }
}

/// 标准 TUI 启动参数：指定工作区 + bypass 权限（UI 测试不需要权限拦截）。
pub fn default_args() -> Vec<&'static str> {
    vec!["-C", workspace(), "--permission-mode", "bypass"]
}

/// 短临界区锁：仅用于"扫描 `/proc` + 信号残留 workspace 进程"互斥。
///
/// Phase 2 起 `PtySession` 不再持此锁——会话期间允许并发；只在每个测试
/// `finish_in_dir` 收尾时短暂持有，避免两个并发收尾同时扫 `/proc` 行为
/// 重复 + 信号同一 pid 二次。
fn cleanup_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 跳过 workspace trust gate（首次运行时显示的信任确认界面）。
/// 如果屏幕上出现 "trust" 相关文本，按 Enter 接受。
pub fn skip_trust_gate(session: &PtySession) {
    // 给 TUI 一些时间渲染
    std::thread::sleep(Duration::from_millis(500));
    let screen = session.current_screen();
    if screen.contains("trust") || screen.contains("Trust") || screen.contains("safety check") {
        session.send_raw(b"\r");
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 快速超时：UI 渲染等待。
pub const RENDER_WAIT: Duration = Duration::from_secs(3);

/// 快速超时：退出等待。
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(10);

/// TUI 退出确认等待。
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(3);

/// timeout 后强制终止时最多等待 child reap 的时长。
pub const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(2);

/// 双键退出确认之间的间隔。
pub const EXIT_CONFIRM_DELAY: Duration = Duration::from_millis(500);

/// API 超时：网络 + 模型延迟。
pub const API_TIMEOUT: Duration = Duration::from_secs(60);

/// Phase-2 Task 5：cleanup 节流窗——两次 `/proc` 扫描最少间隔 2s。
pub const CLEANUP_THROTTLE: Duration = Duration::from_secs(2);

/// Phase-2 Task 5：被节流跳过的 cleanup 调用次数（测试可读断言）。
pub static CLEANUP_SKIP_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Phase-2 Task 5：find_detached_workspace_processes 实际触发次数（测试用）。
pub static FIND_DETACHED_SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);

static LAST_CLEANUP_SCAN: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();

fn cleanup_scan_is_due(last_scan: &mut Option<Instant>, now: Instant) -> bool {
    if last_scan.is_some_and(|previous| now.saturating_duration_since(previous) < CLEANUP_THROTTLE)
    {
        return false;
    }

    *last_scan = Some(now);
    true
}

// ─── PtySession ──────────────────────────────────────────────────────

/// 伪终端会话：在真实 PTY 中启动二进制，捕获输出，模拟输入。
pub struct PtySession {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    #[cfg(unix)]
    process_group_leader: Option<libc::pid_t>,
    slave: Option<Box<dyn portable_pty::SlavePty + Send>>,
    buffer: Arc<Mutex<Vec<u8>>>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    cols: u16,
    rows: u16,
}

impl PtySession {
    /// 启动 cc-rust TUI 进程。
    ///
    /// - `args`: 命令行参数（不含二进制名）
    /// - `cols`, `rows`: 终端尺寸
    /// - `strip_keys`: 是否清除 API key（离线模式）
    pub fn spawn(args: &[&str], cols: u16, rows: u16, strip_keys: bool) -> Self {
        Self::spawn_with_env(args, cols, rows, strip_keys, &[])
    }

    /// 启动 cc-rust TUI 进程，附加自定义环境变量。
    pub fn spawn_with_env(
        args: &[&str],
        cols: u16,
        rows: u16,
        strip_keys: bool,
        envs: &[(&str, &str)],
    ) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open pty");

        let mut cmd = CommandBuilder::new(binary_path());
        for arg in args {
            cmd.arg(*arg);
        }

        // 离线模式：清除所有 API key
        if strip_keys {
            cmd.env("ANTHROPIC_API_KEY", "");
            cmd.env("AZURE_API_KEY", "");
            cmd.env("OPENAI_API_KEY", "");
            cmd.env("OPENROUTER_API_KEY", "");
            cmd.env("GOOGLE_API_KEY", "");
            cmd.env("DEEPSEEK_API_KEY", "");
        }

        for (key, value) in envs {
            cmd.env(key, value);
        }

        let child = pair.slave.spawn_command(cmd).expect("spawn in pty");
        #[cfg(unix)]
        let process_group_leader = pair.master.process_group_leader();

        // 读写器
        let writer: Box<dyn Write + Send> = pair.master.take_writer().expect("take writer");
        let shared_writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(writer));
        let writer_for_reader = Arc::clone(&shared_writer);

        let mut reader = pair.master.try_clone_reader().expect("clone reader");
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::with_capacity(64 * 1024)));
        let buf_clone = Arc::clone(&buffer);

        // 读取线程：持续捕获 PTY 输出 + 自动回复 DSR 查询
        let reader_thread = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut tail = Vec::with_capacity(16);
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = &chunk[..n];
                        buf_clone.lock().unwrap().extend_from_slice(data);

                        // 自动回复 DSR \x1b[6n → \x1b[1;1R
                        // crossterm 在初始化时会发送此查询，不回复会导致阻塞
                        tail.extend_from_slice(data);
                        while let Some(pos) = find_subsequence(&tail, b"\x1b[6n") {
                            if let Ok(mut w) = writer_for_reader.lock() {
                                let _ = w.write_all(b"\x1b[1;1R");
                                let _ = w.flush();
                            }
                            tail.drain(..pos + 4);
                        }
                        if tail.len() > 16 {
                            let start = tail.len() - 16;
                            tail.drain(..start);
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::BrokenPipe {
                            eprintln!("[pty reader] error: {e}");
                        }
                        break;
                    }
                }
            }
        });

        Self {
            writer: shared_writer,
            child,
            #[cfg(unix)]
            process_group_leader,
            slave: Some(pair.slave),
            buffer,
            reader_thread: Some(reader_thread),
            cols,
            rows,
        }
    }

    // ── 输入模拟 ─────────────────────────────────────────────

    /// 发送原始字节到 PTY stdin。
    pub fn send_raw(&self, bytes: &[u8]) {
        self.try_send_raw(bytes).expect("write raw");
    }

    fn try_send_raw(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("pty writer mutex poisoned"))?;
        w.write_all(bytes)?;
        w.flush()
    }

    fn send_raw_for_teardown(&self, bytes: &[u8], label: &str) {
        if let Err(err) = self.try_send_raw(bytes) {
            eprintln!("[pty] teardown {label} skipped: {err}");
        }
    }

    fn request_confirmed_exit(&self, bytes: &[u8], label: &str) {
        self.send_raw_for_teardown(bytes, label);
        std::thread::sleep(EXIT_CONFIRM_DELAY);
        self.send_raw_for_teardown(bytes, label);
    }

    /// 发送一行文本 + Enter。
    pub fn send_line(&self, line: &str) {
        self.send_raw(format!("{}\r", line).as_bytes());
    }

    /// Ctrl+C (ETX 0x03)。
    pub fn send_ctrl_c(&self) {
        self.send_raw(&[0x03]);
    }

    /// Ctrl+D (EOT 0x04) — 退出。
    pub fn send_ctrl_d(&self) {
        self.send_raw(&[0x04]);
    }

    /// Ctrl+G (BEL 0x07) — vim 模式切换。
    #[allow(dead_code)]
    pub fn send_ctrl_g(&self) {
        self.send_raw(&[0x07]);
    }

    /// Up arrow。
    pub fn send_up(&self) {
        self.send_raw(b"\x1b[A");
    }

    /// Down arrow。
    pub fn send_down(&self) {
        self.send_raw(b"\x1b[B");
    }

    /// Right arrow。
    pub fn send_right(&self) {
        self.send_raw(b"\x1b[C");
    }

    /// Left arrow。
    pub fn send_left(&self) {
        self.send_raw(b"\x1b[D");
    }

    /// Backspace key.
    pub fn send_backspace(&self) {
        self.send_raw(&[0x7f]);
    }

    /// Delete key.
    pub fn send_delete(&self) {
        self.send_raw(b"\x1b[3~");
    }

    /// Page Up key.
    pub fn send_page_up(&self) {
        self.send_raw(b"\x1b[5~");
    }

    /// Page Down key.
    pub fn send_page_down(&self) {
        self.send_raw(b"\x1b[6~");
    }

    /// Home key.
    pub fn send_home(&self) {
        self.send_raw(b"\x1b[H");
    }

    /// End key.
    pub fn send_end(&self) {
        self.send_raw(b"\x1b[F");
    }

    /// Escape 键。
    pub fn send_escape(&self) {
        self.send_raw(&[0x1b]);
    }

    /// Tab 键。
    #[allow(dead_code)]
    pub fn send_tab(&self) {
        self.send_raw(&[0x09]);
    }

    /// Ctrl+U (0x15) — 清除当前行。
    #[allow(dead_code)]
    pub fn send_ctrl_u(&self) {
        self.send_raw(&[0x15]);
    }

    /// Ctrl+L (0x0C) — 清屏。
    #[allow(dead_code)]
    pub fn send_ctrl_l(&self) {
        self.send_raw(&[0x0C]);
    }

    /// Ctrl+R (0x12) — 反向搜索。
    #[allow(dead_code)]
    pub fn send_ctrl_r(&self) {
        self.send_raw(&[0x12]);
    }

    /// F12 键序列 — 触发 TUI debug snapshot。
    #[allow(dead_code)]
    pub fn send_f12(&self) {
        self.send_raw(b"\x1b[24~");
    }

    // ── 屏幕读取 ─────────────────────────────────────────────

    /// 获取当前终端屏幕内容（通过 vt100 仿真）。
    /// 返回的是当前屏幕的可见文本，不是累积输出。
    pub fn current_screen(&self) -> String {
        let buf = self.buffer.lock().unwrap().clone();
        let mut parser = vt100::Parser::new(self.rows, self.cols, 0);
        parser.process(&buf);
        parser.screen().contents()
    }

    /// 读取指定行（0-indexed）。
    #[allow(dead_code)]
    pub fn screen_row(&self, row: u16) -> String {
        self.screen_rows()
            .get(row as usize)
            .cloned()
            .unwrap_or_default()
    }

    fn screen_rows(&self) -> Vec<String> {
        let buf = self.buffer.lock().unwrap().clone();
        let mut parser = vt100::Parser::new(self.rows, self.cols, 0);
        parser.process(&buf);
        let screen = parser.screen();
        let mut rows = Vec::with_capacity(self.rows as usize);
        for row in 0..self.rows {
            let mut line = String::new();
            for col in 0..self.cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                let ch = cell.contents();
                if ch.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(&ch);
                }
            }
            rows.push(line.trim_end().to_string());
        }
        rows
    }

    /// 读取状态栏。
    ///
    /// vt100 的当前 screen buffer 在真实 PTY 下偶尔会把最后一行解析为空，
    /// 但状态栏仍保留在底部附近的有效行中。这里先扫描底部可见行，再从
    /// 累积纯文本中回退提取最近一次状态栏片段，避免 model_flow 测试因为
    /// harness 取错行而跳过状态栏断言。
    pub fn status_bar(&self) -> String {
        let rows = self.screen_rows();
        for line in rows.iter().rev() {
            if is_status_bar_candidate(line) {
                return line.trim().to_string();
            }
        }

        let last_row = rows.last().cloned().unwrap_or_default();
        if !last_row.trim().is_empty() {
            return last_row.trim().to_string();
        }

        extract_status_from_text(&self.current_text()).unwrap_or_default()
    }

    /// 获取已捕获的原始输出（ANSI 去除后的纯文本）。
    pub fn current_text(&self) -> String {
        let buf = self.buffer.lock().unwrap().clone();
        let plain = strip_ansi_escapes::strip(&buf);
        String::from_utf8_lossy(&plain).into_owned()
    }

    // ── 等待断言 ─────────────────────────────────────────────

    /// 等待状态栏包含指定文本。
    #[allow(dead_code)]
    pub fn wait_status(&self, needle: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return false;
            }
            if self.status_bar().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// 等待模型响应完成。
    ///
    /// The current footer no longer renders the old `ready | N msgs` contract.
    /// Treat a response as complete once the TUI has shown a busy state, emitted
    /// additional output after the submitted prompt, and returned to a non-busy
    /// screen.
    pub fn wait_response_done(&self, _min_msgs: usize, timeout: Duration) -> bool {
        let start = Instant::now();
        std::thread::sleep(Duration::from_millis(150));
        let baseline_len = self.current_text().len();
        let mut saw_busy = false;
        let mut saw_output_after_submit = false;

        loop {
            if start.elapsed() > timeout {
                return false;
            }

            let bar = self.status_bar();
            let screen = self.current_screen();
            let busy = response_is_busy(&bar, &screen);
            if busy {
                saw_busy = true;
            }

            if self.current_text().len() > baseline_len.saturating_add(8) {
                saw_output_after_submit = true;
            }

            if !busy
                && saw_output_after_submit
                && (saw_busy || start.elapsed() > Duration::from_secs(2))
            {
                return true;
            }

            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// 等待输出中出现指定文本（ANSI 去除后）。
    pub fn wait_for_text(&self, needle: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return false;
            }
            if self.current_text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// 等待屏幕中出现指定文本。
    #[allow(dead_code)]
    pub fn wait_for_screen_text(&self, needle: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return false;
            }
            if self.current_screen().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// 等待任意一个文本出现，返回匹配的索引。
    pub fn wait_for_any(&self, needles: &[&str], timeout: Duration) -> Option<usize> {
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return None;
            }
            let text = self.current_text();
            for (i, needle) in needles.iter().enumerate() {
                if text.contains(needle) {
                    return Some(i);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // ── 截图与日志 ───────────────────────────────────────────

    /// Mid-session 截图：保存 .log / .html，返回纯文本。
    pub fn snapshot(&self, label: &str) -> String {
        let raw = self.buffer.lock().unwrap().clone();
        let plain = strip_ansi_escapes::strip(&raw);
        let output = CapturedOutput {
            raw: raw.clone(),
            plain: plain.clone(),
            cols: self.cols,
            rows: self.rows,
        };
        let paths = write_capture_files(logs_dir(), label, &output);

        eprintln!(
            "[snapshot] {label}: {} bytes → {}",
            output.raw.len(),
            paths.html.display()
        );
        String::from_utf8_lossy(&plain).into_owned()
    }

    /// Mid-session 截图保存到指定目录。
    pub fn snapshot_to(&self, label: &str, dir: &std::path::Path) -> String {
        let raw = self.buffer.lock().unwrap().clone();
        let plain = strip_ansi_escapes::strip(&raw);
        let output = CapturedOutput {
            raw: raw.clone(),
            plain: plain.clone(),
            cols: self.cols,
            rows: self.rows,
        };
        let paths = write_capture_files(dir, label, &output);

        eprintln!(
            "[snapshot] {label}: {} bytes → {}",
            output.raw.len(),
            paths.html.display()
        );
        String::from_utf8_lossy(&plain).into_owned()
    }

    // ── 结束会话 ─────────────────────────────────────────────

    /// 等待子进程退出，保存日志，返回捕获的输出。
    pub fn finish(self, timeout: Duration, test_name: &str) -> CapturedOutput {
        self.finish_in_dir(timeout, test_name, logs_dir())
    }

    /// 通过双 Ctrl+C 请求退出，短暂等待后保存日志。
    pub fn finish_after_quit(self, test_name: &str) -> CapturedOutput {
        self.finish_after_quit_to(test_name, logs_dir())
    }

    /// 通过双 Ctrl+C 请求退出，短暂等待后保存到指定目录。
    pub fn finish_after_quit_to(self, test_name: &str, dir: &std::path::Path) -> CapturedOutput {
        self.request_confirmed_exit(&[0x03], "Ctrl+C");
        self.finish_in_dir(EXIT_TIMEOUT, test_name, dir)
    }

    /// 通过双 Ctrl+D 请求退出，短暂等待后保存到指定目录。
    pub fn finish_after_ctrl_d_to(self, test_name: &str, dir: &std::path::Path) -> CapturedOutput {
        self.request_confirmed_exit(&[0x04], "Ctrl+D");
        self.finish_in_dir(EXIT_TIMEOUT, test_name, dir)
    }

    /// 等待子进程退出并保存到指定目录。
    pub fn finish_to(
        self,
        timeout: Duration,
        test_name: &str,
        dir: &std::path::Path,
    ) -> CapturedOutput {
        self.finish_in_dir(timeout, test_name, dir)
    }

    fn finish_in_dir(
        mut self,
        timeout: Duration,
        test_name: &str,
        dir: &std::path::Path,
    ) -> CapturedOutput {
        let mut child_exited = self.wait_for_child_exit(timeout);
        if !child_exited {
            eprintln!("[pty] timeout after {timeout:.0?} — terminating child");
            self.terminate_child();
            child_exited = self.wait_for_child_exit(KILL_REAP_TIMEOUT);
            if !child_exited {
                eprintln!(
                    "[pty] child did not exit within {KILL_REAP_TIMEOUT:.0?} after termination"
                );
            }
        }
        cleanup_detached_workspace_processes();

        let (output, paths) = self.capture_output_after_finish(test_name, dir);

        eprintln!(
            "[pty] {test_name}: {} bytes raw, {} bytes plain → {}",
            output.raw.len(),
            output.plain.len(),
            paths.log.display()
        );

        output
    }

    fn wait_for_child_exit(&mut self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(err) => {
                    eprintln!("[pty] try_wait failed: {err}");
                    return true;
                }
            }

            if start.elapsed() >= timeout {
                return false;
            }
        }
    }

    fn terminate_child(&mut self) {
        #[cfg(unix)]
        self.terminate_process_group();

        if let Err(err) = self.child.kill() {
            eprintln!("[pty] child kill failed: {err}");
        }
    }

    #[cfg(unix)]
    fn terminate_process_group(&self) {
        let Some(pgid) = self.process_group_to_kill() else {
            eprintln!("[pty] no child process group available; falling back to child kill");
            return;
        };

        let current_pgid = unsafe { libc::getpgrp() };
        if pgid == current_pgid {
            eprintln!("[pty] refusing to signal current process group {pgid}");
            return;
        }

        let result = unsafe { libc::kill(-pgid, libc::SIGTERM) };
        if result != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                eprintln!("[pty] process group kill failed for {pgid}: {err}");
            }
        }
    }

    #[cfg(unix)]
    fn process_group_to_kill(&self) -> Option<libc::pid_t> {
        self.process_group_leader
            .or_else(|| {
                self.child
                    .process_id()
                    .and_then(|pid| libc::pid_t::try_from(pid).ok())
            })
            .filter(|pid| *pid > 0)
    }

    fn capture_output_after_finish(
        mut self,
        test_name: &str,
        dir: &std::path::Path,
    ) -> (CapturedOutput, CapturePaths) {
        // Phase-2 Task 5：从 200ms → 100ms 收尾 sleep（reader 线程已 endpoint-flush）。
        std::thread::sleep(Duration::from_millis(100));
        drop(self.slave.take());
        drop(self.writer);
        if let Some(h) = self.reader_thread.take() {
            let start = Instant::now();
            // Phase-2 Task 5：reader join deadline 从 500ms → 250ms（多数情况下
            // reader 在 endpoint 收到 Eof 后 < 50ms 退出）。
            while !h.is_finished() && start.elapsed() < Duration::from_millis(250) {
                std::thread::sleep(Duration::from_millis(20));
            }
            if h.is_finished() {
                let _ = h.join();
            } else {
                eprintln!("[pty] reader thread still active; leaving it detached");
            }
        }

        let raw = self.buffer.lock().unwrap().clone();
        let plain = strip_ansi_escapes::strip(&raw);

        let output = CapturedOutput {
            raw,
            plain,
            cols: self.cols,
            rows: self.rows,
        };
        let paths = write_capture_files(dir, test_name, &output);

        (output, paths)
    }
}

// ─── CapturedOutput ──────────────────────────────────────────────────

/// 测试捕获的终端输出。
pub struct CapturedOutput {
    pub raw: Vec<u8>,
    pub plain: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
}

impl CapturedOutput {
    /// 纯文本内容。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.plain).into_owned()
    }

    /// 是否包含指定文本。
    pub fn contains(&self, needle: &str) -> bool {
        self.text().contains(needle)
    }

    /// 打印预览（最多 max_bytes 字节）。
    #[allow(dead_code)]
    pub fn preview(&self, max_bytes: usize) {
        let text = self.text();
        let end = text
            .char_indices()
            .take_while(|(i, _)| *i < max_bytes)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(text.len());
        eprintln!("[preview]\n{}", &text[..end]);
    }

    /// 当前终端屏幕内容，每一行对应一个 vt100 屏幕行。
    pub fn screen_text(&self) -> String {
        let parser = self.parser_for_visible_screen();
        let screen = parser.screen();
        let mut rows = Vec::with_capacity(self.rows as usize);
        for row in 0..self.rows {
            let mut line = String::new();
            let mut col = 0u16;
            while col < self.cols {
                let Some(cell) = screen.cell(row, col) else {
                    col += 1;
                    continue;
                };
                let ch = cell.contents();
                let display = if ch.is_empty() { " " } else { &ch };
                line.push_str(display);
                let width = unicode_width::UnicodeWidthStr::width(display);
                col += if width > 1 { width as u16 } else { 1 };
            }
            rows.push(line.trim_end().to_string());
        }
        rows.join("\n")
    }

    fn render_html_with_meta(&self, meta: &HtmlMeta<'_>) -> String {
        let parser = self.parser_for_visible_screen();
        let screen = parser.screen();
        let generated_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let title = html_escape(meta.title);
        let label = html_escape(meta.label);
        let generated_at = html_escape(&generated_at);

        let mut html = String::with_capacity(self.raw.len() * 3);
        html.push_str(&format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{title}</title>
<style>
body{{background:#1e1e1e;color:#ddd;margin:0;padding:16px;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}}
.wrap{{display:flex;flex-direction:column;align-items:center;gap:12px}}
.meta{{box-sizing:border-box;width:max-content;max-width:100%;min-width:min(100%,720px);background:#151515;border:1px solid #3a3a3a;border-radius:8px;padding:10px 12px;color:#d6d6d6;font-size:13px;line-height:1.5}}
.meta strong{{color:#fff}}
.meta code{{font-family:'Cascadia Code','Consolas','Courier New',monospace;color:#f2f2f2}}
.terminal{{background:#0c0c0c;border:1px solid #444;border-radius:8px;padding:12px;box-shadow:0 4px 24px rgba(0,0,0,.5);overflow:auto;max-width:100%}}
pre{{font-family:'Cascadia Code','Consolas','Courier New',monospace;font-size:14px;line-height:1.3;margin:0;color:#ccc}}
.row{{display:block;height:1.3em;white-space:pre}}
</style></head><body><div class="wrap">
<div class="meta"><strong>{title}</strong><br>
label: <code>{label}</code><br>
terminal: <code>{cols}x{rows}</code> | raw bytes: <code>{raw_len}</code> | plain bytes: <code>{plain_len}</code> | generated: <code>{generated_at}</code></div>
<div class="terminal"><pre>
"#,
            cols = self.cols,
            rows = self.rows,
            raw_len = self.raw.len(),
            plain_len = self.plain.len(),
        ));

        for row in 0..self.rows {
            html.push_str("<span class=\"row\">");
            let mut col = 0u16;
            while col < self.cols {
                let cell = screen.cell(row, col).unwrap();
                let ch = cell.contents();
                let fg = color_to_css(cell.fgcolor());
                let bg = color_to_css(cell.bgcolor());
                let (fg_css, bg_css) = if cell.inverse() {
                    (
                        bg.as_deref().unwrap_or("#0c0c0c"),
                        fg.as_deref().unwrap_or("#cccccc"),
                    )
                } else {
                    (
                        fg.as_deref().unwrap_or("#cccccc"),
                        bg.as_deref().unwrap_or("#0c0c0c"),
                    )
                };

                let mut style = String::new();
                if fg_css != "#cccccc" || cell.inverse() {
                    style.push_str(&format!("color:{fg_css};"));
                }
                if bg_css != "#0c0c0c" || cell.inverse() {
                    style.push_str(&format!("background:{bg_css};"));
                }
                if cell.bold() {
                    style.push_str("font-weight:bold;");
                }
                if cell.underline() {
                    style.push_str("text-decoration:underline;");
                }

                let display = if ch.is_empty() { " " } else { &ch };
                let escaped = html_escape(display);
                if style.is_empty() {
                    html.push_str(&escaped);
                } else {
                    html.push_str(&format!("<span style=\"{style}\">{escaped}</span>"));
                }
                let width = unicode_width::UnicodeWidthStr::width(display);
                col += if width > 1 { width as u16 } else { 1 };
            }
            html.push_str("</span>\n");
        }

        html.push_str("</pre></div></div></body></html>");
        html
    }

    fn parser_for_visible_screen(&self) -> vt100::Parser {
        let mut parser = vt100::Parser::new(self.rows, self.cols, 0);
        parser.process(&self.raw);

        // 如果屏幕被退出清理清空了，回退到最后一个有效帧
        if parser.screen().contents().trim().is_empty() {
            let markers: &[&[u8]] = &[b"\x1b[?1049l", b"\x1b[2J"];
            let mut best_end = self.raw.len();
            for m in markers {
                if let Some(pos) = rfind(&self.raw, m) {
                    best_end = best_end.min(pos);
                }
            }
            let home_seq = b"\x1b[H";
            let mut search_end = self.raw.len();
            while let Some(pos) = rfind(&self.raw[..search_end], home_seq) {
                let mut test_parser = vt100::Parser::new(self.rows, self.cols, 0);
                test_parser.process(&self.raw[..pos]);
                if !test_parser.screen().contents().trim().is_empty() {
                    best_end = best_end.min(pos);
                    break;
                }
                search_end = pos;
            }
            parser = vt100::Parser::new(self.rows, self.cols, 0);
            parser.process(&self.raw[..best_end]);
        }

        parser
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

struct CapturePaths {
    log: PathBuf,
    html: PathBuf,
}

struct HtmlMeta<'a> {
    title: &'a str,
    label: &'a str,
}

fn write_capture_files(dir: &Path, label: &str, output: &CapturedOutput) -> CapturePaths {
    let log_path = dir.join(format!("{label}.log"));
    let stream_log_path = dir.join(format!("{label}.stream.log"));
    let raw_path = dir.join(format!("{label}.raw"));
    let html_path = dir.join(format!("{label}.html"));

    std::fs::write(&log_path, output.screen_text()).expect("write screen log");
    std::fs::write(&stream_log_path, &output.plain).expect("write stream log");
    std::fs::write(&raw_path, &output.raw).expect("write raw log");
    let html = output.render_html_with_meta(&HtmlMeta {
        title: "PTY TUI E2E Capture",
        label,
    });
    std::fs::write(&html_path, html.as_bytes()).expect("write html");

    CapturePaths {
        log: log_path,
        html: html_path,
    }
}

/// 从状态栏解析 "N msgs" 中的 N。
pub fn parse_msg_count(status: &str) -> Option<usize> {
    for word in status.split_whitespace() {
        if let Ok(n) = word.parse::<usize>() {
            if status.contains(&format!("{n} msgs")) || status.contains(&format!("{n} msg")) {
                return Some(n);
            }
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

fn color_to_css(color: vt100::Color) -> Option<String> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(idx_to_css(i)),
        vt100::Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

fn idx_to_css(i: u8) -> String {
    const PALETTE: [&str; 16] = [
        "#0c0c0c", "#c50f1f", "#13a10e", "#c19c00", "#0037da", "#881798", "#3a96dd", "#cccccc",
        "#767676", "#e74856", "#16c60c", "#f9f1a5", "#3b78ff", "#b4009e", "#61d6d6", "#f2f2f2",
    ];
    if (i as usize) < PALETTE.len() {
        return PALETTE[i as usize].to_string();
    }
    if i >= 232 {
        let v = 8 + (i - 232) as u32 * 10;
        let v = v.min(255) as u8;
        return format!("#{v:02x}{v:02x}{v:02x}");
    }

    let idx = (i - 16) as u32;
    let r = idx / 36;
    let g = (idx % 36) / 6;
    let b = idx % 6;
    let to_val = |c: u32| if c == 0 { 0u8 } else { (55 + c * 40) as u8 };
    format!("#{:02x}{:02x}{:02x}", to_val(r), to_val(g), to_val(b))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_status_bar_candidate(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('>') || trimmed.contains("Message cc-rust") {
        return false;
    }

    let has_model_path = trimmed.contains(" | ");
    let has_status = trimmed.contains("ready")
        || trimmed.contains("BUSY")
        || trimmed.contains("INS")
        || trimmed.contains(" msgs")
        || trimmed.contains(" msg");
    has_model_path || has_status
}

fn response_is_busy(status_bar: &str, screen: &str) -> bool {
    status_bar.contains("BUSY")
        || status_bar.contains("tab to queue")
        || screen.contains("Thinking")
        || screen.contains("BUSY")
        || screen.contains("tab to queue")
}

fn extract_status_from_text(text: &str) -> Option<String> {
    let workspace = workspace();
    let idx = text.rfind(workspace)?;
    let prefix_start = text[..idx]
        .rfind(['\n', '\r', '│', '┘'])
        .map(|pos| pos + 1)
        .unwrap_or(0);
    let suffix_end = text[idx..]
        .find(['\n', '\r', '>', '│'])
        .map(|offset| idx + offset)
        .unwrap_or_else(|| text.len());
    let candidate = text[prefix_start..suffix_end].trim();
    (!candidate.is_empty()).then(|| candidate.to_string())
}

#[cfg(unix)]
#[derive(Clone, Copy)]
struct WorkspaceProcess {
    pid: libc::pid_t,
    pgid: Option<libc::pid_t>,
}

#[cfg(unix)]
fn cleanup_detached_workspace_processes() {
    // Phase 2：仅在 cleanup 短临界区持锁；不再包住整个会话。
    let _guard = cleanup_lock();
    // Phase 2 Task 5：节流 — 若距上次扫描不足 CLEANUP_THROTTLE，跳过 /proc 扫描。
    let last = LAST_CLEANUP_SCAN.get_or_init(|| std::sync::Mutex::new(None));
    let scan_is_due = {
        let mut last_scan = last.lock().expect("last cleanup scan poisoned");
        cleanup_scan_is_due(&mut last_scan, Instant::now())
    };
    if !scan_is_due {
        CLEANUP_SKIP_COUNT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let targets = find_detached_workspace_processes();
    if targets.is_empty() {
        return;
    }

    for target in &targets {
        signal_workspace_process(*target, libc::SIGTERM);
    }

    let start = Instant::now();
    while start.elapsed() < KILL_REAP_TIMEOUT {
        if targets
            .iter()
            .all(|target| !workspace_process_is_alive(target.pid))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    for target in &targets {
        if workspace_process_is_alive(target.pid) {
            signal_workspace_process(*target, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn cleanup_detached_workspace_processes() {}

#[cfg(unix)]
fn find_detached_workspace_processes() -> Vec<WorkspaceProcess> {
    FIND_DETACHED_SCAN_COUNT.fetch_add(1, Ordering::Relaxed);
    let workspace = workspace();
    let current_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Ok(pid_u32) = name.parse::<u32>() else {
            continue;
        };
        if pid_u32 == current_pid {
            continue;
        }

        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if !workspace_cmdline_matches(&cmdline, workspace) {
            continue;
        }

        let Ok(pid) = libc::pid_t::try_from(pid_u32) else {
            continue;
        };
        let pgid = match unsafe { libc::getpgid(pid) } {
            pgid if pgid > 0 => Some(pgid),
            _ => None,
        };
        processes.push(WorkspaceProcess { pid, pgid });
    }

    processes
}

#[cfg(unix)]
fn workspace_cmdline_matches(cmdline: &[u8], workspace: &str) -> bool {
    if cmdline.is_empty() {
        return false;
    }

    let cmdline = String::from_utf8_lossy(cmdline);
    cmdline.contains(workspace)
        && (cmdline.contains("mcp-cli-daemon") || cmdline.contains("mcp-cli-bridge"))
}

#[cfg(unix)]
fn signal_workspace_process(target: WorkspaceProcess, signal: libc::c_int) {
    let current_pgid = unsafe { libc::getpgrp() };
    if let Some(pgid) = target.pgid.filter(|pgid| *pgid != current_pgid) {
        let _ = unsafe { libc::kill(-pgid, signal) };
    } else {
        let _ = unsafe { libc::kill(target.pid, signal) };
    }
}

#[cfg(unix)]
fn workspace_process_is_alive(pid: libc::pid_t) -> bool {
    if pid <= 0 {
        return false;
    }

    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_path_preserves_explicit_override() {
        assert_eq!(
            resolve_workspace_path(Some("/tmp/custom-pty-workspace"), 1234),
            "/tmp/custom-pty-workspace"
        );
    }

    #[test]
    fn default_workspace_path_is_scoped_by_runner_pid() {
        let first = resolve_workspace_path(None, 1234);
        let second = resolve_workspace_path(None, 5678);

        assert_eq!(first, "/tmp/cc-rust-e2e-test-1234");
        assert_eq!(second, "/tmp/cc-rust-e2e-test-5678");
        assert_ne!(first, second);
    }

    #[test]
    fn cleanup_throttle_scans_first_call_then_throttles() {
        let start = Instant::now();
        let mut last_scan = None;

        assert!(cleanup_scan_is_due(&mut last_scan, start));
        assert!(!cleanup_scan_is_due(
            &mut last_scan,
            start + Duration::from_millis(50)
        ));
        assert!(cleanup_scan_is_due(
            &mut last_scan,
            start + CLEANUP_THROTTLE
        ));
    }

    /// Phase-2 Task 4 Step 1：验证 `PtySession::spawn` 不再被全局 lock 串行化。
    ///
    /// 同时让两个线程各自 `PtySession::spawn` 一个 cc-rust 实例，在 spawn 之后立即
    /// 把"PtySession 还活着"计数登记进 `Arc<AtomicUsize>`；`overlap ≥ 2` 即代表
    /// 两个会话在时间窗内重叠。Phase 1 时 `_serial_guard` 把整个会话串行化，
    /// 此 test 红；Phase 2 起 cleanup_lock 只在收尾临界区持锁，此 test 应绿。
    #[test]
    fn concurrent_sessions_do_not_serialize() {
        let alive = Arc::new(AtomicUsize::new(0));
        let overlap = Arc::new(AtomicUsize::new(0));

        let args = default_args();
        let spawn_thread = move |_alive: Arc<AtomicUsize>, _overlap: Arc<AtomicUsize>| {
            let alive = _alive;
            let overlap = _overlap;
            let args = args.clone();
            thread::spawn(move || {
                let session = PtySession::spawn(&args, 80, 24, true);
                let cur = alive.fetch_add(1, Ordering::SeqCst) + 1;
                if cur > 1 {
                    overlap.fetch_max(cur, Ordering::SeqCst);
                }
                // Hold ~700ms — overlap window
                std::thread::sleep(Duration::from_millis(700));
                drop(session);
                alive.fetch_sub(1, Ordering::SeqCst);
            })
        };

        let h1 = spawn_thread(Arc::clone(&alive), Arc::clone(&overlap));
        // Tiny offset so thread-1 has entered spawn() first
        std::thread::sleep(Duration::from_millis(50));
        let h2 = spawn_thread(Arc::clone(&alive), Arc::clone(&overlap));

        h1.join().expect("t1");
        h2.join().expect("t2");

        let max_overlap = overlap.load(Ordering::SeqCst);
        assert!(
            max_overlap >= 2,
            "concurrent sessions were serialized by the global pty lock; \
             max overlap observed = {max_overlap}, expected ≥ 2"
        );
    }

    /// Phase-2 Task 5：验证 cleanup 调用被节流。
    ///
    /// 调用 5 次连续 `cleanup_detached_workspace_processes()`（每次间隔 50ms，
    /// 总耗时 250ms < 节流窗 2s）。若节流生效，至少后 4 次跳过 /proc 扫描
    /// （`CLEANUP_SKIP_COUNT` ≥ 4），实际 `find_detached_workspace_processes`
    /// 触发次数 ≤ 1。
    ///
    /// 注意：测试进程级的 throttle 与测试顺序敏感——若其他 test 在测试间
    /// 留下 < 2s 的扫描时间戳，本断言仍应成立（连续 5 次只多 1 次扫描就够）。
    #[cfg(unix)]
    #[test]
    fn cleanup_detached_workspace_processes_throttles() {
        let before_skip = CLEANUP_SKIP_COUNT.load(Ordering::Relaxed);
        let before_scan = FIND_DETACHED_SCAN_COUNT.load(Ordering::Relaxed);

        for _ in 0..5 {
            cleanup_detached_workspace_processes();
            std::thread::sleep(Duration::from_millis(50));
        }

        let after_skip = CLEANUP_SKIP_COUNT.load(Ordering::Relaxed);
        let after_scan = FIND_DETACHED_SCAN_COUNT.load(Ordering::Relaxed);

        let skipped = after_skip.saturating_sub(before_skip);
        let scans = after_scan.saturating_sub(before_scan);

        assert!(
            skipped >= 4,
            "cleanup was not throttled: skipped {skipped}/5 calls, expected ≥ 4"
        );
        assert!(
            scans <= 1,
            "cleanup scanned /proc too many times: {scans}, expected ≤ 1"
        );
    }

    #[test]
    fn status_candidate_accepts_model_path_line() {
        assert!(is_status_bar_candidate("gpt-5.4 | /tmp/cc-rust-e2e-test"));
    }

    #[test]
    fn status_candidate_rejects_prompt_line() {
        assert!(!is_status_bar_candidate(
            ">   Message cc-rust, / for commands  [INS]"
        ));
    }
}
