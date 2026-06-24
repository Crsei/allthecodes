//! Image generation placeholder handler.
//!
//! Returns a stub response indicating no image generation provider is configured.
//! Ready for real provider integration (DALL-E, Stable Diffusion, etc.).

use async_trait::async_trait;
use axum::extract::State;
use axum::response::Response;
use axum::routing::post;
use axum::Json;

use allthecodes_protocol::v1::image_generate::{ImageGenerateRequest, ImageGenerateResponse};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ImageGenerateProcessor {
    state: WebState,
}

impl From<WebState> for ImageGenerateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for ImageGenerateProcessor {
    type Request = ImageGenerateRequest;
    type Response = ImageGenerateResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "image_generate"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _req: Self::Request) -> Result<Self::Response, Self::Error> {
        Ok(ImageGenerateResponse {
            image_id: String::new(),
            url: None,
            base64: None,
            revised_prompt: None,
            status: "error".into(),
            error: Some(
                "No image generation provider configured. \
                 Set `image_generate_provider` in settings to enable."
                    .into(),
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge
// ---------------------------------------------------------------------------

async fn generate_handler(
    State(state): State<WebState>,
    Json(req): Json<ImageGenerateRequest>,
) -> Response {
    rest_processor_response::<ImageGenerateProcessor>(state, ApiMethod::ImageGenerate, req).await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::ImageGenerate, post(generate_handler))
}
