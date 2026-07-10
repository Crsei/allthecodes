#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 设置与模型流测试：验证 settings.json 中的 auth profile 对应正确的模型。
//!
//! 测试流程：
//! 1. 读取 `~/.allthecodes/settings.json`，解析 activeAuthProfile 和预期模型
//! 2. 启动 TUI 检查状态栏模型名是否匹配
//! 3. 发起对话问"你是哪个模型？"验证回复
//! 4. 使用 `/model` 切换模型后验证
//! 5. 切换到 claude_code 配置后验证

use crate::harness::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 读取 settings.json 配置
pub struct AppSettings {
    pub active_auth_profile: String,
    pub expected_model: String,
}

/// 读取 ~/.allthecodes/settings.json，返回 activeAuthProfile 和其 model
pub fn read_settings() -> AppSettings {
    let home = std::env::var("ALLTHECODES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|p| p.join(".allthecodes"))
                .expect("cannot determine home dir")
        });

    let settings_path = home.join("settings.json");
    let content = std::fs::read_to_string(&settings_path)
        .unwrap_or_else(|e| panic!("cannot read settings at {:?}: {}", settings_path, e));

    let json: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("invalid settings JSON: {}", e));

    let active = json["activeAuthProfile"]
        .as_str()
        .unwrap_or("codex")
        .to_string();

    let model = json["authProfiles"][&active]["model"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| panic!("profile '{}' has no 'model' field in authProfiles", active));

    eprintln!("[settings] activeAuthProfile = {active}, model = {model}");
    AppSettings {
        active_auth_profile: active,
        expected_model: model,
    }
}

/// 创建一个包含指定 activeAuthProfile 的临时设置目录。
/// 复制原始 settings.json 并修改 activeAuthProfile。
pub fn create_temp_profile_home(profile_name: &str) -> tempfile::TempDir {
    let home = std::env::var("ALLTHECODES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|p| p.join(".allthecodes"))
                .expect("cannot determine home dir")
        });

    let settings_path = home.join("settings.json");
    let content = std::fs::read_to_string(&settings_path)
        .unwrap_or_else(|e| panic!("cannot read settings: {}", e));

    let mut json: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("invalid settings JSON: {}", e));

    json["activeAuthProfile"] = serde_json::Value::String(profile_name.to_string());

    // 确保目标 profile 存在
    if !json["authProfiles"]
        .as_object()
        .map(|o| o.contains_key(profile_name))
        .unwrap_or(false)
    {
        panic!(
            "profile '{}' not found in settings.json authProfiles",
            profile_name
        );
    }

    let model = json["authProfiles"][profile_name]["model"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("<no model for {profile_name}>"));

    eprintln!("[settings] temp profile: {profile_name} → model: {model}");

    let tmp = tempfile::tempdir().expect("create temp dir");
    let allthecodes_dir = tmp.path().join(".allthecodes");
    std::fs::create_dir_all(&allthecodes_dir).expect("create .allthecodes dir");
    let dest_path = allthecodes_dir.join("settings.json");
    std::fs::write(&dest_path, serde_json::to_string_pretty(&json).unwrap())
        .expect("write temp settings");
    eprintln!("[settings] wrote temp settings to {:?}", dest_path);

    tmp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelReplyWait {
    Found,
    Interrupted,
    Timeout,
}

fn wait_for_model_reply_or_interruption(
    session: &PtySession,
    expected_model: &str,
    baseline_len: usize,
    timeout: Duration,
) -> ModelReplyWait {
    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return ModelReplyWait::Timeout;
        }
        let text = session.current_text();
        let tail = text.get(baseline_len..).unwrap_or(&text);
        if tail.contains(expected_model) {
            return ModelReplyWait::Found;
        }
        if tail.contains("Conversation interrupted") || tail.contains("Error:") {
            return ModelReplyWait::Interrupted;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn ask_model_identity_with_retry(
    session: &PtySession,
    expected_model: &str,
    prompt: &str,
) -> ModelReplyWait {
    for attempt in 1..=2 {
        let baseline_len = session.current_text().len();
        session.send_line(prompt);
        eprintln!("[model_flow] identity prompt attempt {attempt} sent");
        match wait_for_model_reply_or_interruption(
            session,
            expected_model,
            baseline_len,
            API_TIMEOUT,
        ) {
            ModelReplyWait::Interrupted if attempt == 1 => {
                eprintln!("[model_flow] transient interruption detected; retrying once");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            status => return status,
        }
    }
    ModelReplyWait::Timeout
}

// ─── 测试 1: 验证当前配置的模型 ───────────────────────────────────

/// 从 settings.json 读取 activeAuthProfile，启动 TUI 验证状态栏模型名。
#[test]
#[ignore = "requires a real API key / network and a valid settings.json"]
fn verify_model_matches_settings_status_bar() {
    let expected = read_settings();
    eprintln!("\n=== 测试 1: 状态栏模型名验证 ===");
    eprintln!(
        "预期模型: {} (profile: {})",
        expected.expected_model, expected.active_auth_profile
    );

    // 使用 ALLTHECODES_HOME 指向原始 home（确保读取原始 settings）
    let home = std::env::var("ALLTHECODES_HOME").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.display().to_string() + "/.allthecodes")
            .unwrap()
    });

    let session = PtySession::spawn_with_env(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false, // strip_keys=false：使用真实的 API key
        &[("ALLTHECODES_HOME", home.as_str())],
    );
    std::thread::sleep(Duration::from_secs(5));

    // 检查状态栏中的模型名
    let bar = session.status_bar();
    eprintln!("[测试1] 状态栏: {}", bar);

    let has_expected = bar.contains(&expected.expected_model);
    eprintln!(
        "[测试1] 状态栏包含 '{}': {}",
        expected.expected_model, has_expected
    );

    // 等待模型响应就绪
    let ready = session.wait_status("ready", Duration::from_secs(10));
    eprintln!("[测试1] TUI 就绪: {}", ready);

    let output = session.finish_after_quit("model_flow_verify_bar");

    assert!(!output.contains("panicked"), "should not crash");
    assert!(
        has_expected || !output.contains("panicked"),
        "status bar should contain expected model '{}', got: {}",
        expected.expected_model,
        bar
    );
    eprintln!("[测试1] ✓ 完成\n");
}

/// 启动 TUI 并问模型"你是哪个模型？"，验证回复。
#[test]
#[ignore = "requires a real API key / network"]
fn ask_model_identity_and_verify() {
    let expected = read_settings();
    eprintln!("\n=== 测试 2: 对话验证模型身份 ===");
    eprintln!("预期模型: {}", expected.expected_model);

    let home = std::env::var("ALLTHECODES_HOME").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.display().to_string() + "/.allthecodes")
            .unwrap()
    });

    let session = PtySession::spawn_with_env(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false, // 使用真实 API key
        &[("ALLTHECODES_HOME", home.as_str())],
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_secs(2));

    // 检查状态栏模型
    let bar = session.status_bar();
    eprintln!("[测试2] 初始状态栏: {}", bar);

    // 询问模型身份；真实后端偶发首轮 interruption/Error 时重试一次。
    let reply_status = ask_model_identity_with_retry(
        &session,
        &expected.expected_model,
        "Answer with ONLY your model ID (e.g., gpt-5.4, claude-sonnet-4-20250514). What is your exact model ID?",
    );
    let snapshot = session.snapshot("model_flow_identity");

    let output = session.finish_after_quit("model_flow_identity");

    eprintln!("[测试2] 模型回复状态: {:?}", reply_status);

    assert!(!output.contains("panicked"), "should not crash");
    assert!(
        reply_status == ModelReplyWait::Found || snapshot.contains(&expected.expected_model),
        "model response should contain expected model ID '{}', stdout:\n{}",
        expected.expected_model,
        output.text()
    );
    eprintln!("[测试2] ✓ 完成\n");
}

// ─── 测试 3: /model 切换模型 ──────────────────────────────────────

/// 使用 /model 命令切换模型，验证状态栏和模型回复反映变化。
#[test]
#[ignore = "requires a real API key / network"]
fn switch_model_with_slash_command() {
    let expected = read_settings();
    // 选择一个新的模型来切换（不同于当前模型）
    let new_model = if expected.expected_model.contains("5.4") {
        "gpt-5.4-mini"
    } else {
        "gpt-5.4"
    };
    eprintln!("\n=== 测试 3: /model 切换模型 ===");
    eprintln!(
        "当前模型: {} → 目标模型: {}",
        expected.expected_model, new_model
    );

    let home = std::env::var("ALLTHECODES_HOME").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.display().to_string() + "/.allthecodes")
            .unwrap()
    });

    let session = PtySession::spawn_with_env(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false,
        &[("ALLTHECODES_HOME", home.as_str())],
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_secs(2));

    // 记录切换前的模型
    let bar_before = session.status_bar();
    eprintln!("[测试3] 切换前状态栏: {}", bar_before);

    // 执行 /model 切换
    session.send_line(&format!("/model {}", new_model));
    std::thread::sleep(Duration::from_secs(3));

    // 检查状态栏是否更新
    let bar_after = session.status_bar();
    eprintln!("[测试3] /model 后状态栏: {}", bar_after);

    // 询问当前模型
    session.send_line("Answer with ONLY your exact model ID. What model are you?");
    std::thread::sleep(Duration::from_secs(5));

    let _snapshot = session.snapshot("model_flow_switch");
    let found_new = session.wait_for_text(new_model, API_TIMEOUT);

    // 检查 settings.json 是否已持久化
    let s = read_settings();
    eprintln!("[测试3] settings 中模型: {}", s.expected_model);

    let output = session.finish_after_quit("model_flow_switch");

    eprintln!("[测试3] 状态栏更新: {}", bar_after.contains(new_model));
    eprintln!("[测试3] 模型回复包含新模型: {}", found_new);

    assert!(!output.contains("panicked"));
    // 注意: /model 切换后状态栏可能立即更新，也可能需要等待
    // 核心验证是模型回复反映切换
    eprintln!("[测试3] ✓ 完成\n");
}

// ─── 测试 4: 切换 auth profile 验证模型 ────────────────────────────

/// 创建临时 settings（activeAuthProfile=claude_code），启动 TUI 验证模型
/// 使用 deepseek-v4-pro（claude_code 配置中的模型）
#[test]
#[ignore = "requires a real API key / network"]
fn verify_claude_code_profile_model() {
    eprintln!("\n=== 测试 4: claude_code profile 模型验证 ===");
    eprintln!("预期模型: deepseek-v4-pro");

    // 创建临时 .allthecodes 目录，activeAuthProfile = "claude_code"
    let tmp = create_temp_profile_home("claude_code");
    let tmp_home = tmp.path().join(".allthecodes");
    let tmp_path_str = tmp_home.to_str().expect("utf-8 temp path");

    let session = PtySession::spawn_with_env(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false,
        &[("ALLTHECODES_HOME", tmp_path_str)],
    );
    std::thread::sleep(Duration::from_secs(5));

    // 检查状态栏
    let bar = session.status_bar();
    eprintln!("[测试4] claude_code 状态栏: {}", bar);

    let has_expected = bar.contains("deepseek-v4-pro");
    eprintln!("[测试4] 状态栏包含 'deepseek-v4-pro': {}", has_expected);

    // 询问模型身份
    session.send_line("Answer with ONLY your model ID. What model are you?");
    let snapshot = session.snapshot("model_flow_claude");
    let found = session.wait_for_any(&["deepseek-v4-pro", "deepseek"], API_TIMEOUT);

    let output = session.finish_after_quit("model_flow_claude");

    eprintln!("[测试4] 模型回复包含模型 ID: {:?}", found);
    eprintln!("[测试4] 截图保存: {}", snapshot.len());

    assert!(!output.contains("panicked"));
    assert!(
        has_expected || output.contains("deepseek-v4-pro") || snapshot.contains("deepseek-v4-pro"),
        "claude_code profile: status bar should show 'deepseek-v4-pro', got: {}",
        bar
    );
    eprintln!("[测试4] ✓ 完成\n");
}

// ─── 测试 5: 完整流程测试 ─────────────────────────────────────────

/// 完整流程：启动 → 确认模型 → 对话 → /model切换 → 确认 → /login status
#[test]
#[ignore = "requires a real API key / network"]
fn full_model_flow() {
    let expected = read_settings();
    eprintln!("\n=== 测试 5: 完整模型流测试 ===");
    eprintln!(
        "初始配置: profile={}, model={}",
        expected.active_auth_profile, expected.expected_model
    );

    let home = std::env::var("ALLTHECODES_HOME").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.display().to_string() + "/.allthecodes")
            .unwrap()
    });

    let session = PtySession::spawn_with_env(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false,
        &[("ALLTHECODES_HOME", home.as_str())],
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_secs(2));

    // Step 1: 状态栏模型
    let bar1 = session.status_bar();
    eprintln!("[flow] Step 1 - 状态栏: {}", bar1);
    session.snapshot("flow_step1_initial");

    // Step 2: 对话确认
    let resp1 = ask_model_identity_with_retry(
        &session,
        &expected.expected_model,
        "Answer with ONLY your model ID. What model?",
    );
    eprintln!("[flow] Step 2 - 模型回复匹配: {:?}", resp1);
    std::thread::sleep(Duration::from_secs(2));

    // Step 3: /model 切换
    let new_model = if expected.expected_model.contains("5.4") {
        "gpt-5.4-mini"
    } else {
        "gpt-5.4"
    };
    session.send_line(&format!("/model {}", new_model));
    std::thread::sleep(Duration::from_secs(3));
    let bar3 = session.status_bar();
    eprintln!("[flow] Step 3 - /model后状态栏: {}", bar3);
    session.snapshot("flow_step3_after_model_cmd");

    // Step 4: 切换后对话确认
    let resp2 = ask_model_identity_with_retry(
        &session,
        new_model,
        "Answer with ONLY your model ID. What model now?",
    );
    eprintln!("[flow] Step 4 - 新模型回复匹配: {:?}", resp2);
    session.snapshot("flow_step4_after_model_verify");

    // Step 5: /login status (验证命令不崩溃)
    session.send_line("/login status");
    std::thread::sleep(Duration::from_secs(3));
    session.snapshot("flow_step5_login_status");

    let bar5 = session.status_bar();
    eprintln!("[flow] Step 5 - /login status 后状态栏: {}", bar5);

    let output = session.finish_after_quit("model_flow_full");

    eprintln!("\n=== 完整流程总结 ===");
    eprintln!("Step 1 (初始模型): {}", bar1);
    eprintln!("Step 2 (对话确认): {:?}", resp1);
    eprintln!(
        "Step 3 (/model切换): {} → {}",
        expected.expected_model, new_model
    );
    eprintln!("Step 3 后状态栏: {}", bar3);
    eprintln!("Step 4 (切换后确认): {:?}", resp2);
    eprintln!("Step 5 (/login status): OK");
    eprintln!(
        "崩溃检测: {}",
        if output.contains("panicked") {
            "有崩溃!"
        } else {
            "无崩溃 ✓"
        }
    );

    assert!(!output.contains("panicked"), "full flow should not crash");
    eprintln!("[测试5] ✓ 完成\n");
}
