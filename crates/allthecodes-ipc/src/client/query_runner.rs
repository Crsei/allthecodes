//! Query-turn client path helper.

use std::sync::Arc;

pub use allthecodes_types::query_host::QueryTurnHost;
use futures::{Stream, StreamExt};
use parking_lot::Mutex;
use tracing::error;

use crate::transport::FrontendSink;

/// Spawn a query turn as a background task and map each SDK message to IPC.
pub fn spawn_query_turn<E, Svc, SdkMessage, F>(
    engine: Arc<E>,
    prompt_text: String,
    message_id: String,
    suggestion_svc: Arc<Mutex<Svc>>,
    sink: FrontendSink,
    mut handle_sdk_message: F,
) where
    E: QueryTurnHost<SdkMessage>,
    E::Stream: Stream<Item = SdkMessage> + Send + 'static,
    Svc: Send + 'static,
    SdkMessage: Send + 'static,
    F: FnMut(&SdkMessage, &str, &Arc<E>, &Arc<Mutex<Svc>>, &FrontendSink) -> std::io::Result<()>
        + Send
        + 'static,
{
    tokio::spawn(async move {
        engine.reset_abort();

        let stream = engine.submit_client_message(&prompt_text);
        let mut stream = std::pin::pin!(stream);

        while let Some(sdk_msg) = stream.next().await {
            if let Err(err) =
                handle_sdk_message(&sdk_msg, &message_id, &engine, &suggestion_svc, &sink)
            {
                error!("query_runner: send to frontend failed: {}", err);
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_ipc_protocol::BackendMessage;
    use futures::stream;

    #[derive(Default)]
    struct MockQueryHost {
        events: Mutex<Vec<String>>,
    }

    impl QueryTurnHost<String> for MockQueryHost {
        type Stream = stream::Iter<std::vec::IntoIter<String>>;

        fn reset_abort(&self) {
            self.events.lock().push("reset".to_string());
        }

        fn submit_client_message(&self, prompt_text: &str) -> Self::Stream {
            self.events.lock().push(format!("submit:{prompt_text}"));
            stream::iter(vec!["first".to_string(), "second".to_string()])
        }
    }

    #[tokio::test]
    async fn spawn_query_turn_resets_submits_and_maps_stream_in_order() {
        let engine = Arc::new(MockQueryHost::default());
        let suggestion_svc = Arc::new(Mutex::new(()));
        let sink = FrontendSink::memory();

        spawn_query_turn(
            engine.clone(),
            "hello".to_string(),
            "msg-1".to_string(),
            suggestion_svc,
            sink.clone(),
            |sdk_msg, message_id, engine, _suggestions, sink| {
                engine
                    .events
                    .lock()
                    .push(format!("handle:{message_id}:{sdk_msg}"));
                sink.send(&BackendMessage::SystemInfo {
                    text: format!("{message_id}:{sdk_msg}"),
                    level: "info".to_string(),
                })
            },
        );

        wait_for_messages(&sink, 2).await;

        assert_eq!(
            engine.events.lock().as_slice(),
            [
                "reset",
                "submit:hello",
                "handle:msg-1:first",
                "handle:msg-1:second",
            ]
        );
        let captured = sink.captured();
        assert!(matches!(
            &captured[0],
            BackendMessage::SystemInfo { text, .. } if text == "msg-1:first"
        ));
        assert!(matches!(
            &captured[1],
            BackendMessage::SystemInfo { text, .. } if text == "msg-1:second"
        ));
    }

    async fn wait_for_messages(sink: &FrontendSink, expected: usize) {
        for _ in 0..50 {
            if sink.captured().len() >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("timed out waiting for query messages");
    }
}
