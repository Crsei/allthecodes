#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 截图与日志保存测试：验证 HTML 终端截图和快照功能。

use crate::harness::*;
use std::time::Duration;

/// Mid-session snapshot 应保存 HTML 文件
#[test]
fn snapshot_saves_html_file() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    // 输入一些内容使屏幕有东西
    session.send_raw(b"hello snapshot test");
    std::thread::sleep(Duration::from_millis(500));

    let text = session.snapshot("screenshot_test");

    let _output = session.finish_after_quit("screenshot_finish");

    // 验证快照返回了文本
    assert!(!text.is_empty(), "snapshot should return text content");

    // 验证 HTML 文件存在
    let html_path = logs_dir().join("screenshot_test.html");
    assert!(html_path.exists(), "HTML screenshot should be saved");
    let html = std::fs::read_to_string(&html_path).expect("read HTML");
    assert!(html.contains("<!DOCTYPE html>"), "should be valid HTML");
    assert!(html.contains("terminal"), "should have terminal CSS class");
}

/// finish 应保存 .raw / .log / .html 三种格式
#[test]
fn finish_saves_all_log_formats() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let _output = session.finish_after_quit("screenshot_formats");

    let dir = logs_dir();
    let raw_path = dir.join("screenshot_formats.raw");
    let log_path = dir.join("screenshot_formats.log");
    let html_path = dir.join("screenshot_formats.html");

    assert!(raw_path.exists(), ".raw log should be saved");
    assert!(log_path.exists(), ".log file should be saved");
    assert!(html_path.exists(), ".html screenshot should be saved");

    // .raw 应包含 ANSI 转义序列（字节数 > 0）
    let raw = std::fs::read(&raw_path).expect("read raw");
    assert!(!raw.is_empty(), ".raw should not be empty");

    // .log 应是纯文本
    let log = std::fs::read_to_string(&log_path).expect("read log");
    assert!(!log.is_empty(), ".log should not be empty");

    // .html 应是有效的 HTML
    let html = std::fs::read_to_string(&html_path).expect("read html");
    assert!(html.contains("</html>"), "should be complete HTML");
}

/// 多次 snapshot 不冲突（不同 label）
#[test]
fn multiple_snapshots_with_different_labels() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    session.snapshot("snap_step1");
    session.send_raw(b"step 2 input");
    std::thread::sleep(Duration::from_millis(300));
    session.snapshot("snap_step2");

    let _output = session.finish_after_quit("screenshot_multi");

    let dir = logs_dir();
    assert!(dir.join("snap_step1.html").exists());
    assert!(dir.join("snap_step2.html").exists());
}
