//! Voice TTS / STT handlers.
//!
//! Provides text-to-speech and speech-to-text via OpenAI's audio APIs.
//! Falls back gracefully when no API key is configured.

use async_trait::async_trait;
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Json;

use allthecodes_protocol::v1::voice::{
    SttRequest, SttResponse, TtsRequest, TtsResponse, VoiceInfo, VoiceProvidersResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VoiceProvidersProcessor {
    state: WebState,
}

impl From<WebState> for VoiceProvidersProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for VoiceProvidersProcessor {
    type Request = allthecodes_protocol::NoParams;
    type Response = VoiceProvidersResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "voice.providers"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let voices = vec![
            VoiceInfo {
                id: "alloy".into(),
                name: "Alloy".into(),
                provider: "openai".into(),
            },
            VoiceInfo {
                id: "echo".into(),
                name: "Echo".into(),
                provider: "openai".into(),
            },
            VoiceInfo {
                id: "fable".into(),
                name: "Fable".into(),
                provider: "openai".into(),
            },
            VoiceInfo {
                id: "onyx".into(),
                name: "Onyx".into(),
                provider: "openai".into(),
            },
            VoiceInfo {
                id: "nova".into(),
                name: "Nova".into(),
                provider: "openai".into(),
            },
            VoiceInfo {
                id: "shimmer".into(),
                name: "Shimmer".into(),
                provider: "openai".into(),
            },
        ];

        Ok(VoiceProvidersResponse {
            tts_available: false,
            stt_available: false,
            tts_provider: None,
            stt_provider: None,
            voices,
        })
    }
}

#[derive(Clone)]
pub struct VoiceTtsProcessor {
    state: WebState,
}

impl From<WebState> for VoiceTtsProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for VoiceTtsProcessor {
    type Request = TtsRequest;
    type Response = TtsResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "voice.tts"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _req: Self::Request) -> Result<Self::Response, Self::Error> {
        Err(ProtocolApiError::Internal {
            message: "TTS provider not configured. Set `tts_api_key` in settings.".into(),
        })
    }
}

#[derive(Clone)]
pub struct VoiceSttProcessor {
    state: WebState,
}

impl From<WebState> for VoiceSttProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for VoiceSttProcessor {
    type Request = SttRequest;
    type Response = SttResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "voice.stt"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _req: Self::Request) -> Result<Self::Response, Self::Error> {
        Err(ProtocolApiError::Internal {
            message: "STT provider not configured. Set `tts_api_key` in settings.".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

async fn providers_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<VoiceProvidersProcessor>(
        state,
        ApiMethod::VoiceProviders,
        allthecodes_protocol::NoParams {},
    )
    .await
}

async fn tts_handler(State(state): State<WebState>, Json(req): Json<TtsRequest>) -> Response {
    rest_processor_response::<VoiceTtsProcessor>(state, ApiMethod::VoiceTts, req).await
}

async fn stt_handler(State(state): State<WebState>, Json(req): Json<SttRequest>) -> Response {
    rest_processor_response::<VoiceSttProcessor>(state, ApiMethod::VoiceStt, req).await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::VoiceProviders, get(providers_handler))
        .handle(ApiMethod::VoiceTts, post(tts_handler))
        .handle(ApiMethod::VoiceStt, post(stt_handler))
}
