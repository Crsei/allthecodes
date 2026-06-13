//! Processor infrastructure for protocol-driven API handlers.

use allthecodes_protocol::{ApiError as ProtocolApiError, NoParams, SerializationScope};
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::Instrument;

use crate::serialization::SerializationLayer;
use crate::state::WebState;

#[async_trait]
pub trait Processor: Clone + Send + Sync + 'static {
    type Request: Send + 'static;
    type Response: Serialize + Send + 'static;
    type Error: Into<ProtocolApiError> + Send + 'static;

    fn handler_name() -> &'static str;

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        None
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::Concurrent
    }

    fn serialization_key(_params: &Self::Request) -> String {
        String::new()
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error>;
}

pub async fn processor_json_handler<P>(
    State(state): State<WebState>,
    Json(params): Json<P::Request>,
) -> Response
where
    P: Processor + From<WebState>,
    P::Request: DeserializeOwned,
{
    process_processor(P::from(state), params).await
}

pub async fn processor_no_params_handler<P>(State(state): State<WebState>) -> Response
where
    P: Processor<Request = NoParams> + From<WebState>,
{
    process_processor(P::from(state), NoParams {}).await
}

pub async fn processor_path_handler<P>(
    AxumPath(params): AxumPath<P::Request>,
    State(state): State<WebState>,
) -> Response
where
    P: Processor + From<WebState>,
    P::Request: DeserializeOwned,
{
    process_processor(P::from(state), params).await
}

pub async fn process_processor<P>(processor: P, params: P::Request) -> Response
where
    P: Processor,
{
    match dispatch_processor(processor, params).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(error),
    }
}

pub async fn dispatch_processor<P>(
    processor: P,
    params: P::Request,
) -> Result<P::Response, ProtocolApiError>
where
    P: Processor,
{
    let span = tracing::info_span!("api.processor", handler = P::handler_name());
    async move {
        let scope = P::serialization_scope(&params);
        let key = P::serialization_key(&params);
        if let Some(layer) = processor.serialization_layer() {
            layer
                .run_scoped(&scope, key, || {
                    let processor = processor.clone();
                    async move { processor.handle(params).await }
                })
                .await
                .map_err(Into::into)
        } else {
            processor.handle(params).await.map_err(Into::into)
        }
    }
    .instrument(span)
    .await
}

pub(crate) fn protocol_error_response(error: ProtocolApiError) -> Response {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body())).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Clone)]
    struct FakeProcessor;

    #[derive(Debug, Deserialize)]
    struct FakeRequest {
        value: String,
    }

    #[derive(Serialize)]
    struct FakeResponse {
        value: String,
    }

    #[async_trait]
    impl Processor for FakeProcessor {
        type Request = FakeRequest;
        type Response = FakeResponse;
        type Error = ProtocolApiError;

        fn handler_name() -> &'static str {
            "fake"
        }

        async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
            if params.value.is_empty() {
                return Err(ProtocolApiError::Validation {
                    field: "value".to_string(),
                    message: "must not be empty".to_string(),
                });
            }
            Ok(FakeResponse {
                value: params.value,
            })
        }
    }

    #[tokio::test]
    async fn processor_success_converts_to_json_response() {
        let response = process_processor(
            FakeProcessor,
            FakeRequest {
                value: "ok".to_string(),
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn processor_error_uses_protocol_status() {
        let response = process_processor(
            FakeProcessor,
            FakeRequest {
                value: String::new(),
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
