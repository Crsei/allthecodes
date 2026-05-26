//! API client -- creates provider-specific HTTP clients and drives the
//! Anthropic Messages API (streaming + non-streaming).

mod types;
mod headers;
mod body;
mod model;
mod provider;
mod builder;
mod messages;
mod stream;
#[cfg(test)]
mod tests;

pub use types::*;
pub(crate) use headers::*;
pub use body::*;
pub use model::*;
pub(crate) use provider::*;
pub(crate) use stream::parse_sse_byte_stream;
#[cfg(test)]
use stream::parse_sse_text;

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
