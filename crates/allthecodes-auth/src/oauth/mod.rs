//! OAuth 2.0 Authorization Code + PKCE login flow.
//!
//! Supports two modes:
//! - **Claude.ai**: Bearer token for Pro/Max subscribers
//! - **Console**: Creates an API key via OAuth
//! - **OpenAI Codex**: ChatGPT OAuth token for Codex backend
//!
//! The login flow is two-step (driven by `/login` and `/login-code` commands):
//! 1. `/login 2|3|4` → generates PKCE, prints auth URL
//! 2. `/login-code <code>` → exchanges code for tokens, stores them
//!
//! Token auto-refresh is handled by `auth::resolve_auth()` when it detects an
//! expired token on disk.

pub mod client;
pub mod config;
pub mod pkce;

pub use config::OAuthMethod;
