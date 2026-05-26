//! OpenAI-compatible provider - handles all providers using the
//! OpenAI chat/completions API format.
//!
//! Covers: OpenAI, DeepSeek, Groq, OpenRouter, Qwen, Zhipu, Moonshot,
//! Baichuan, MiniMax, Yi, SiliconFlow, StepFun, Spark.
//!
//! Converts our internal MessagesRequest (Anthropic format) to OpenAI format,
//! sends the streaming request, and parses the SSE response back into our
//! StreamEvent type so the rest of the system (StreamAccumulator, etc.) works
//! unchanged.
//!
//! Reference: code-iris/crates/iris-llm/src/openai.rs

pub(crate) use chat_stream::openai_compat_stream;

mod builder;
mod chat_stream;
mod codex;
mod format;
