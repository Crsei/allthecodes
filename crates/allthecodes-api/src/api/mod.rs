//! API Client 鈥?multi-provider LLM integration.
//!
//! Supports: Anthropic (native), OpenAI-compatible (15+ providers),
//! Google Gemini, AWS Bedrock (Claude), GCP Vertex AI (Claude).

pub mod bedrock;
pub mod client;
pub mod google_provider;
pub mod openai_compat;
pub mod pricing;
pub mod providers;
pub mod retry;
pub mod sigv4;
pub mod stream_provider;
pub mod streaming;
pub mod vertex;
