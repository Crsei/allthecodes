//! Tests: JSON-RPC request cancellation before ACK.
//!
//! Verifies that cancelling a `session/prompt` request via `$/cancel_request`
//! before the prompt ACK is sent results in the prompt being cancelled and
//! no engine stream being started.

mod support;

use std::sync::Arc;

use agent_client_protocol_schema::v2;
use agent_client_protocol_schema::ProtocolVersion;
use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::AcpEngineParams;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use serial_test::serial;

use support::{is_response, RuntimeHarness};

/// A factory that creates a real QueryEngine for test prompting.
pub struct TestEngineFactory;

impl AcpEngineFactory for TestEngineFactory {
    fn create_engine(&self, params: AcpEngineParams) -> anyhow::Result<Arc<QueryEngine>> {
        let cwd = params.cwd.to_string_lossy().to_string();
        let config = QueryEngineConfig {
            cwd,
            tools: vec![],
            max_turns: Some(1),
            initial_messages: params.initial_messages,
            verbose: false,
            resolved_model: Some("test-model".to_string()),
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            auto_save_session: false,
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_budget_usd: None,
            task_budget: None,
            agent_context: None,
        };
        Ok(Arc::new(QueryEngine::new(config)))
    }
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn set_str(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct AuthEnv {
    _temp: tempfile::TempDir,
    _guards: Vec<EnvGuard>,
}

type TestKeychain = std::collections::HashMap<(String, String), Vec<u8>>;
static TEST_KEYCHAIN: std::sync::OnceLock<std::sync::Mutex<TestKeychain>> =
    std::sync::OnceLock::new();

#[derive(Debug)]
struct TestCredential {
    service: String,
    user: String,
}

impl keyring::credential::CredentialApi for TestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .insert((self.service.clone(), self.user.clone()), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .get(&(self.service.clone(), self.user.clone()))
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .remove(&(self.service.clone(), self.user.clone()))
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct TestCredentialBuilder;

impl keyring::credential::CredentialBuilderApi for TestCredentialBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<keyring::Credential>> {
        Ok(Box::new(TestCredential {
            service: service.to_string(),
            user: user.to_string(),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> keyring::credential::CredentialPersistence {
        keyring::credential::CredentialPersistence::ProcessOnly
    }
}

fn use_test_keyring() {
    TEST_KEYCHAIN
        .get_or_init(Default::default)
        .lock()
        .expect("test keychain poisoned")
        .clear();
    keyring::set_default_credential_builder(Box::new(TestCredentialBuilder));
}

fn isolated_auth_env() -> AuthEnv {
    use_test_keyring();
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("allthecodes-home");
    let user_home = temp.path().join("user-home");
    std::fs::create_dir_all(&data_home).unwrap();
    std::fs::create_dir_all(&user_home).unwrap();

    AuthEnv {
        _temp: temp,
        _guards: vec![
            EnvGuard::set_path("ALLTHECODES_HOME", &data_home),
            EnvGuard::set_path("HOME", &user_home),
            EnvGuard::remove("ANTHROPIC_API_KEY"),
            EnvGuard::remove("ANTHROPIC_AUTH_TOKEN"),
            EnvGuard::remove("OPENAI_API_KEY"),
            EnvGuard::remove("OPENAI_CODEX_AUTH_TOKEN"),
        ],
    }
}

fn initialize_params() -> serde_json::Value {
    serde_json::to_value(v2::InitializeRequest::new(
        ProtocolVersion::V2,
        v2::Implementation::new("test-client", "0.0.0"),
    ))
    .unwrap()
}

fn auth_methods(response: &serde_json::Value) -> Vec<serde_json::Value> {
    response
        .pointer("/result/authMethods")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
}

async fn initialize_response() -> serde_json::Value {
    let _env = isolated_auth_env();
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );
    let (response, _pre_response) = harness
        .send_request_and_capture("initialize", Some(initialize_params()))
        .await;
    response.expect("initialize should write a response")
}

/// Test: sending `$/cancel_request` for a prompt request id before
/// the prompt's engine task starts should cancel the prompt and
/// return `RequestCancelled` rather than starting engine execution.
///
/// Current broken behaviour: the engine task is spawned immediately
/// and sends state=running before the main loop checks cancellation.
#[tokio::test]
async fn cancel_request_cancels_pending_prompt_before_ack() {
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    // 1. session/new — create a session so we can prompt it.
    let new_params = serde_json::json!({
        "cwd": std::env::current_dir().unwrap(),
        "additionalDirectories": [],
        "mcpServers": {},
    });
    let (new_resp, _pre) = harness
        .send_request_and_capture("session/new", Some(new_params))
        .await;
    assert!(new_resp.is_some(), "session/new should succeed");
    let session_id = new_resp
        .as_ref()
        .unwrap()
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .and_then(|v| v.as_str())
        .expect("missing sessionId")
        .to_string();

    // 2. session/prompt — submit a prompt.
    let prompt_params = serde_json::json!({
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": "Hello"}]
    });
    let messages = harness
        .send_request_cancelled_before_response("session/prompt", Some(prompt_params))
        .await;

    let response = messages
        .iter()
        .find(|message| is_response(message))
        .expect("cancelled prompt should write a JSON-RPC response");
    let error_code = response
        .pointer("/error/code")
        .and_then(|value| value.as_i64());
    assert_eq!(
        error_code,
        Some(-32800),
        "cancelled prompt should return RequestCancelled, got: {response:?}"
    );
    assert!(
        !messages.iter().any(support::is_session_update),
        "cancelled prompt must not start the engine stream: {messages:?}"
    );
}

#[tokio::test]
#[serial]
async fn initialize_advertises_agent_login_when_unauthenticated() {
    let _env = isolated_auth_env();
    let _invalid_key = EnvGuard::set_str("ANTHROPIC_API_KEY", "not-a-valid-key");
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    let (response, _pre_response) = harness
        .send_request_and_capture("initialize", Some(initialize_params()))
        .await;
    let response = response.expect("initialize should write a response");
    let methods = auth_methods(&response);

    assert_eq!(methods.len(), 1, "unexpected auth methods: {response:?}");
    assert_eq!(
        methods[0].get("type").and_then(|value| value.as_str()),
        Some("agent")
    );
    assert_eq!(
        methods[0].get("id").and_then(|value| value.as_str()),
        Some("allthecodes-login")
    );
}

#[tokio::test]
#[serial]
async fn initialize_omits_agent_login_when_authenticated() {
    let _env = isolated_auth_env();
    let _token = EnvGuard::set_str("ANTHROPIC_AUTH_TOKEN", "test-auth-token");
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    let (response, _pre_response) = harness
        .send_request_and_capture("initialize", Some(initialize_params()))
        .await;
    let response = response.expect("initialize should write a response");

    assert_eq!(auth_methods(&response), Vec::<serde_json::Value>::new());
}

#[tokio::test]
#[serial]
async fn auth_login_returns_login_instructions() {
    let _env = isolated_auth_env();
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    let params = serde_json::to_value(v2::LoginAuthRequest::new("allthecodes-login")).unwrap();
    let (response, _pre_response) = harness
        .send_request_and_capture("auth/login", Some(params))
        .await;
    let response = response.expect("auth/login should write a response");
    let instructions = response
        .pointer("/result/_meta/instructions")
        .and_then(|value| value.as_str())
        .expect("auth/login should return instructions in _meta");

    assert!(instructions.contains("/login"), "{instructions}");
    assert!(instructions.contains("/login-code"), "{instructions}");
    assert!(instructions.contains("allthecodes"), "{instructions}");
}

#[tokio::test]
#[serial]
async fn auth_login_rejects_unknown_method() {
    let _env = isolated_auth_env();
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    let params = serde_json::to_value(v2::LoginAuthRequest::new("unknown-login")).unwrap();
    let (response, _pre_response) = harness
        .send_request_and_capture("auth/login", Some(params))
        .await;
    let response = response.expect("auth/login should write a response");

    assert!(
        response.get("error").is_some(),
        "unknown method should fail: {response:?}"
    );
}

#[tokio::test]
#[serial]
async fn auth_logout_is_idempotent() {
    let _env = isolated_auth_env();
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );
    let params = serde_json::to_value(v2::LogoutAuthRequest::new()).unwrap();

    for _ in 0..2 {
        let (response, _pre_response) = harness
            .send_request_and_capture("auth/logout", Some(params.clone()))
            .await;
        let response = response.expect("auth/logout should write a response");
        assert!(
            response.get("error").is_none(),
            "auth/logout should be idempotent: {response:?}"
        );
    }
}

#[tokio::test]
#[serial]
async fn capability_initialize_advertises_session_delete_after_delete_tests_pass() {
    let response = initialize_response().await;

    assert!(
        response
            .pointer("/result/capabilities/session/delete")
            .is_some(),
        "session.delete should be advertised after delete behavior is verified: {response:?}"
    );
}

#[tokio::test]
#[serial]
async fn capability_initialize_omits_session_mcp_until_enabled() {
    let response = initialize_response().await;

    assert!(
        response
            .pointer("/result/capabilities/session/mcp")
            .is_none(),
        "session.mcp must remain unadvertised: {response:?}"
    );
}

#[tokio::test]
#[serial]
async fn capability_initialize_omits_prompt_multimodal_until_enabled() {
    let response = initialize_response().await;

    assert!(
        response
            .pointer("/result/capabilities/session/prompt/image")
            .is_none(),
        "prompt.image must remain unadvertised: {response:?}"
    );
    assert!(
        response
            .pointer("/result/capabilities/session/prompt/audio")
            .is_none(),
        "prompt.audio must remain unadvertised: {response:?}"
    );
    assert!(
        response
            .pointer("/result/capabilities/session/prompt/embeddedContext")
            .is_none(),
        "prompt.embeddedContext must remain unadvertised: {response:?}"
    );
}
