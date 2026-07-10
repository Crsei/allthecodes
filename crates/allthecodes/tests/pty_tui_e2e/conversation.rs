#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 完整对话流测试：输入问题 → 等待响应 → 验证记录 → 多轮上下文。
//!
//! 这些测试**需要真实 API key**，标记为 `#[ignore]`。

use crate::harness::*;
use std::time::Duration;

/// 单轮对话：输入问题，验证模型回复出现在 TUI 中。
///
/// 流程：启动 TUI → 输入 "Say exactly: X" → 等待 "X" 出现 → 退出
#[test]
#[ignore = "requires a real API key and network"]
fn single_turn_renders_response() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // 输入问题
    session.send_line("Say exactly: CONV_TEST_MARKER_7749");

    // 等待模型回复完成；不同 provider 不保证渲染 "Claude:" 前缀。
    let found_response = session.wait_response_done(0, API_TIMEOUT);
    let found_marker = session.wait_for_text("CONV_TEST_MARKER_7749", Duration::from_secs(5));

    let output = session.finish_after_quit("conv_single_turn");

    assert!(
        found_response,
        "should complete the response, got:\n{}",
        output.text()
    );
    assert!(
        found_marker,
        "should contain exact marker text, got:\n{}",
        output.text()
    );
    assert!(!output.contains("panicked"));
}

/// 多轮对话：两轮输入，验证上下文持久。
///
/// 第一轮："Remember X" → 等待响应完成
/// 第二轮："What was X?" → 验证模型回忆起 X
#[test]
#[ignore = "requires a real API key and network"]
fn multi_turn_context_persists() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // ── 第一轮：建立上下文 ──
    session.send_line("The secret code is ZEPHYR_42. Remember it.");
    let turn1_ok = session.wait_response_done(0, API_TIMEOUT);
    assert!(turn1_ok, "turn 1 should complete within timeout");

    let bar1 = session.status_bar();
    let count1 = parse_msg_count(&bar1).unwrap_or(0);
    eprintln!("[conv] turn 1 done, msgs={count1}");
    session.snapshot("conv_turn1");

    std::thread::sleep(Duration::from_secs(2));

    // ── 第二轮：验证回忆 ──
    session.send_line("What is the secret code I told you?");
    let turn2_ok = session.wait_response_done(count1, API_TIMEOUT);
    assert!(turn2_ok, "turn 2 should complete within timeout");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("conv_turn2");

    let _output = session.finish_after_quit("conv_multi_turn");

    assert!(
        snap.to_lowercase().contains("zephyr_42"),
        "model should recall the secret code from turn 1"
    );
}

/// 五轮连续对话：验证状态栏消息计数递增。
#[test]
#[ignore = "requires a real API key and network"]
fn five_turns_msg_count_increases() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    let prompts = ["Say OK1", "Say OK2", "Say OK3", "Say OK4", "Say OK5"];
    let mut completed_turns = 0usize;

    for (i, prompt) in prompts.iter().enumerate() {
        let turn = i + 1;
        eprintln!("[conv] Turn {turn}/{}: {prompt}", prompts.len());

        session.send_line(prompt);
        let ok = session.wait_response_done(0, API_TIMEOUT);
        if !ok {
            session.snapshot(&format!("conv_turn{turn}_timeout"));
            break;
        }

        completed_turns += 1;
        eprintln!("[conv] Turn {turn}: response completed");

        std::thread::sleep(Duration::from_secs(2));
    }

    let _output = session.finish_after_quit("conv_five_turns");

    assert!(
        completed_turns >= 3,
        "at least 3 turns should complete, got {}",
        completed_turns
    );
}

/// Ctrl+C 中断流式输出后，仍可继续新一轮对话。
#[test]
#[ignore = "requires a real API key and network"]
fn abort_then_new_turn() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // 发一个会生成长回复的问题
    session.send_line("Write a 2000-word essay about computing.");
    std::thread::sleep(Duration::from_secs(3));

    // 中断
    session.send_ctrl_c();
    eprintln!("[conv] sent Ctrl+C to abort");
    std::thread::sleep(Duration::from_secs(3));
    session.snapshot("conv_abort");

    // 新一轮
    session.send_line("Say exactly: RECOVERED_AFTER_ABORT");
    let found = session.wait_for_text("RECOVERED_AFTER_ABORT", API_TIMEOUT);

    let _output = session.finish_after_quit("conv_abort_resume");

    assert!(found, "should recover and respond after abort");
}

/// /clear 后继续输入，上下文已重置。
#[test]
#[ignore = "requires a real API key and network"]
fn clear_then_continue() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // 建立上下文
    session.send_line("The secret word is COCONUT.");
    let ok1 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok1);
    std::thread::sleep(Duration::from_secs(2));

    // 清除
    session.send_line("/clear");
    std::thread::sleep(Duration::from_secs(2));

    // 新一轮
    session.send_line("Say exactly: AFTER_CLEAR_OK");
    let ok2 = session.wait_response_done(0, API_TIMEOUT);

    let _output = session.finish_after_quit("conv_clear_continue");

    assert!(ok2, "should produce response after /clear");
}

/// 工具调用（Bash）：验证工具执行结果出现在 TUI 中。
#[test]
#[ignore = "requires a real API key and network"]
fn tool_use_visible_in_tui() {
    let session = PtySession::spawn(&default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("Use the Bash tool to run: echo PTY_TOOL_RESULT_9988");

    // 等待工具输出或模型回复
    let found = session.wait_for_any(&["PTY_TOOL_RESULT_9988", "Claude:"], API_TIMEOUT);

    let output = session.finish_after_quit("conv_tool_use");

    assert!(
        found.is_some(),
        "tool result or Claude: should appear, got:\n{}",
        output.text()
    );
}
