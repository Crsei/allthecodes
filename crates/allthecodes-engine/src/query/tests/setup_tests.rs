use std::sync::Arc;

use futures::StreamExt;

use super::super::*;
use super::mocks::{
    make_query_params, make_text_response, make_user_message_for_test, request_start_count,
    LoopTestTool, MockDeps,
};
use crate::types::app_state::AppState;
use crate::types::config::QuerySource;
use crate::types::message::{Message, MessageContent, QueryYield};
use crate::types::tool::{Tool, Tools};

#[tokio::test]
async fn query_refreshes_tools_before_first_model_call() {
    let refreshed_tool: Arc<dyn Tool> = Arc::new(LoopTestTool {
        name: "mcp__late__fresh",
        concurrency_safe: true,
    });
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")]).with_refreshed_tools(vec![refreshed_tool]),
    );

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("use the late tool")]),
        deps.clone(),
    )
    .collect()
    .await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].tools.len(), 1);
    assert_eq!(recorded[0].tools[0].name(), "mcp__late__fresh");
}

#[tokio::test]
async fn query_shapes_autocompact_with_final_request_context() {
    let refreshed_tool: Arc<dyn Tool> = Arc::new(LoopTestTool {
        name: "mcp__late__fresh",
        concurrency_safe: true,
    });
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")]).with_refreshed_tools(vec![refreshed_tool]),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("count the full request")]);
    params.system_prompt = vec!["system boundary".to_string()];
    params.max_output_tokens_override = Some(1234);
    params.skip_cache_write = Some(true);

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_autocompact_params();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].system_prompt,
        vec!["system boundary".to_string()]
    );
    assert_eq!(recorded[0].tools.len(), 1);
    assert_eq!(recorded[0].tools[0].name(), "mcp__late__fresh");
    assert_eq!(recorded[0].max_output_tokens, Some(1234));
    assert_eq!(recorded[0].skip_cache_write, Some(true));
}

#[tokio::test]
#[serial_test::serial]
async fn deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery() {
    allthecodes_tools::deferred_tools::clear_discovered_tools_for_tests();
    allthecodes_tools::deferred_tools::mark_discovered_tools("unknown", ["WebBrowser".to_string()]);
    let mut tools = allthecodes_tools::deferred_tools::tools();
    tools.push(Arc::new(allthecodes_tools::exec::SleepTool));
    tools.push(Arc::new(LoopTestTool {
        name: "WebBrowser",
        concurrency_safe: true,
    }));
    let deps = Arc::new(MockDeps::new(vec![make_text_response("done")]).with_tools(tools));
    let mut params = make_query_params(vec![make_user_message_for_test(
        "use the discovered browser",
    )]);
    params.gates.deferred_tool_loading = true;

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("SearchExtraTools"));
    assert!(names.contains("ExecuteExtraTool"));
    assert!(names.contains("Sleep"));
    assert!(!names.contains("WebBrowser"));
    assert!(
        allthecodes_tools::deferred_tools::discovered_tools_for_session("unknown")
            .contains("WebBrowser")
    );
}

#[tokio::test]
async fn text_only_model_filters_view_image_from_request_tools() {
    let mut app_state = AppState::default();
    app_state.main_loop_model = "text-only".into();
    app_state.settings.model_capabilities.insert(
        "text-only".into(),
        allthecodes_config::settings::ModelCapabilitySettings {
            input_modalities: vec!["text".into()],
            supports_image_detail_original: false,
            ..Default::default()
        },
    );
    let tools: Tools = vec![
        Arc::new(allthecodes_tools::exec::SleepTool),
        Arc::new(LoopTestTool {
            name: "ViewImage",
            concurrency_safe: true,
        }),
        Arc::new(LoopTestTool {
            name: "view_image",
            concurrency_safe: true,
        }),
    ];
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")])
            .with_tools(tools)
            .with_app_state(app_state),
    );

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("inspect image")]),
        deps.clone(),
    )
    .collect()
    .await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("Sleep"));
    assert!(!names.contains("ViewImage"));
    assert!(!names.contains("view_image"));

    let compact = deps.recorded_autocompact_params();
    assert_eq!(compact.len(), 1);
    let compact_names = compact[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!compact_names.contains("ViewImage"));
    assert!(!compact_names.contains("view_image"));
}

#[tokio::test]
async fn agent_session_filters_prompt_and_recursive_tools_from_request_tools() {
    let tools: Tools = [
        "AskUserQuestion",
        "Agent",
        "Task",
        "TeamSpawn",
        "spawn_agent",
        "FollowupTask",
        "followup_task",
        "Read",
        "SendMessage",
    ]
    .into_iter()
    .map(|name| {
        Arc::new(LoopTestTool {
            name,
            concurrency_safe: true,
        }) as Arc<dyn Tool>
    })
    .collect();
    let deps = Arc::new(MockDeps::new(vec![make_text_response("done")]).with_tools(tools));
    let mut params = make_query_params(vec![make_user_message_for_test("continue agent task")]);
    params.query_source = QuerySource::Agent("child-agent".to_string());

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for hidden in [
        "AskUserQuestion",
        "Agent",
        "Task",
        "TeamSpawn",
        "spawn_agent",
        "FollowupTask",
        "followup_task",
    ] {
        assert!(!names.contains(hidden), "{hidden} should be session-gated");
    }
    assert!(names.contains("Read"));
    assert!(names.contains("SendMessage"));

    let compact = deps.recorded_autocompact_params();
    assert_eq!(compact.len(), 1);
    let compact_names = compact[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!compact_names.contains("AskUserQuestion"));
    assert!(!compact_names.contains("Agent"));
    assert!(!compact_names.contains("Task"));
}

#[tokio::test]
async fn query_drains_steer_before_next_model_request() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response("first answer"),
        make_text_response("steered answer"),
    ]));
    deps.set_steer_drains(vec![vec![], vec!["steer now".to_string()]]);

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    )
    .collect()
    .await;

    assert_eq!(request_start_count(&items), 2);
    assert!(items.iter().any(|item| matches!(
        item,
        QueryYield::Message(Message::User(user))
            if matches!(&user.content, MessageContent::Text(text) if text == "steer now")
    )));

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1].messages.iter().any(|message| matches!(
        message,
        Message::User(user)
            if matches!(&user.content, MessageContent::Text(text) if text == "steer now")
    )));
}
