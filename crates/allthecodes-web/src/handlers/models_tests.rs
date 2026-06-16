use super::*;
use crate::handlers::test_support::*;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn models_set_default_persists_model_setting() {
    let (home, _guard) = temp_home();
    let state = make_web_state();
    state.engine().update_app_state(|s| {
        s.settings.available_models = vec!["gpt-4o".to_string()];
    });

    let response = models_set_default_handler(
        State(state.clone()),
        Json(SetDefaultModelRequest {
            model_id: "gpt-4o".to_string(),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert_eq!(raw.model.as_deref(), Some("gpt-4o"));
    assert_eq!(state.engine().app_state().main_loop_model, "gpt-4o");
}
