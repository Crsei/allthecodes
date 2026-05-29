//! API client -- creates provider-specific HTTP clients and drives the
//! Anthropic Messages API (streaming + non-streaming).

mod body;
mod builder;
mod headers;
mod messages;
mod model;
mod provider;
mod stream;
#[cfg(test)]
mod tests;
mod types;

pub use body::*;
pub(crate) use headers::*;
pub use model::*;
pub(crate) use provider::*;
pub(crate) use stream::parse_sse_byte_stream;
#[cfg(test)]
use stream::parse_sse_text;
pub use types::*;

/// Return true if env var `name` is set to a truthy value (`1`, `true`, `yes`,
/// `on`). Matches claude-code-bun's `isEnvTruthy` semantics.
pub fn is_env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn is_codex_backend(value: &str) -> bool {
    value.eq_ignore_ascii_case("codex")
}
