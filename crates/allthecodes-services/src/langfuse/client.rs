use std::sync::OnceLock;
use std::time::Duration;

use opentelemetry::trace::Tracer;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_langfuse::ExporterBuilder;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::{BatchConfigBuilder, SdkTracer, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use parking_lot::Mutex;

use crate::telemetry::TelemetryEvent;

static LANGFUSE_PROVIDER: OnceLock<Mutex<Option<SdkTracerProvider>>> = OnceLock::new();

pub fn is_langfuse_enabled() -> bool {
    env_var("LANGFUSE_PUBLIC_KEY").is_some() && env_var("LANGFUSE_SECRET_KEY").is_some()
}

pub fn init_langfuse() -> anyhow::Result<Option<SdkTracer>> {
    if !is_langfuse_enabled() {
        return Ok(None);
    }

    let slot = LANGFUSE_PROVIDER.get_or_init(|| Mutex::new(None));
    if let Some(provider) = slot.lock().clone() {
        return Ok(Some(provider.tracer("allthecodes-langfuse")));
    }

    let public_key = env_var("LANGFUSE_PUBLIC_KEY")
        .expect("LANGFUSE_PUBLIC_KEY must exist (checked by is_langfuse_enabled gate)");
    let secret_key = env_var("LANGFUSE_SECRET_KEY")
        .expect("LANGFUSE_SECRET_KEY must exist (checked by is_langfuse_enabled gate)");
    let host = env_var("LANGFUSE_BASE_URL")
        .or_else(|| env_var("LANGFUSE_HOST"))
        .unwrap_or_else(|| "https://cloud.langfuse.com".to_string());
    let timeout_secs = env_parse("LANGFUSE_TIMEOUT").unwrap_or(5);
    let flush_at = env_parse("LANGFUSE_FLUSH_AT").unwrap_or(20).max(1);
    let flush_interval_secs = env_parse("LANGFUSE_FLUSH_INTERVAL").unwrap_or(10).max(1);
    let export_mode = env_var("LANGFUSE_EXPORT_MODE").unwrap_or_else(|| "batched".to_string());
    let environment =
        env_var("LANGFUSE_TRACING_ENVIRONMENT").unwrap_or_else(|| "development".to_string());

    let exporter = ExporterBuilder::new()
        .with_host(&host)
        .with_basic_auth(&public_key, &secret_key)
        .with_timeout(Duration::from_secs(timeout_secs))
        .build()?;

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", "allthecodes"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("deployment.environment", environment),
        ])
        .build();

    let provider = if export_mode.eq_ignore_ascii_case("immediate") {
        SdkTracerProvider::builder()
            .with_resource(resource)
            .with_simple_exporter(exporter)
            .build()
    } else {
        let max_queue_size = (flush_at * 10).max(flush_at + 1);
        let batch_config = BatchConfigBuilder::default()
            .with_max_export_batch_size(flush_at)
            .with_max_queue_size(max_queue_size)
            .with_scheduled_delay(Duration::from_secs(flush_interval_secs))
            .build();

        SdkTracerProvider::builder()
            .with_resource(resource)
            .with_span_processor(
                BatchSpanProcessor::builder(exporter, Tokio)
                    .with_batch_config(batch_config)
                    .build(),
            )
            .build()
    };

    let tracer = provider.tracer("allthecodes-langfuse");
    *slot.lock() = Some(provider);
    Ok(Some(tracer))
}

pub fn shutdown_langfuse() {
    let Some(slot) = LANGFUSE_PROVIDER.get() else {
        return;
    };

    if let Some(provider) = slot.lock().take() {
        if let Err(error) = provider.shutdown() {
            tracing::warn!(error = %error, "failed to shutdown langfuse tracer provider");
        }
    }
}

/// Export telemetry events to Langfuse.
///
/// Converts allthecodes telemetry events to Langfuse-compatible OpenTelemetry
/// spans and sends them through the configured exporter.
pub fn export_telemetry_events(events: Vec<TelemetryEvent>) {
    if events.is_empty() {
        return;
    }

    let slot = match LANGFUSE_PROVIDER.get() {
        Some(slot) => slot,
        None => {
            tracing::debug!("langfuse not initialized, dropping telemetry events");
            return;
        }
    };

    let provider_guard = slot.lock();
    let provider = match provider_guard.as_ref() {
        Some(p) => p,
        None => {
            tracing::debug!("langfuse provider not available, dropping telemetry events");
            return;
        }
    };

    let tracer = provider.tracer("allthecodes-langfuse-telemetry");
    let count = events.len();

    for event in events {
        let (span_name, attrs) = match event {
            TelemetryEvent::Interaction {
                interaction_id,
                duration_ms,
                model,
                error,
                ..
            } => {
                let mut attrs = vec![
                    KeyValue::new("service.name", "allthecodes"),
                    KeyValue::new("event.type", "interaction"),
                ];
                if let Some(ref model) = model {
                    attrs.push(KeyValue::new("llm.model", model.clone()));
                }
                if let Some(dur) = duration_ms {
                    attrs.push(KeyValue::new("duration_ms", dur as i64));
                }
                if let Some(ref error) = error {
                    attrs.push(KeyValue::new("error", error.clone()));
                }
                (format!("interaction:{}", interaction_id), attrs)
            }
            TelemetryEvent::ModelCall {
                span_id,
                model,
                duration_ms,
                request_tokens,
                response_tokens,
                error,
                ..
            } => {
                let mut attrs = vec![
                    KeyValue::new("service.name", "allthecodes"),
                    KeyValue::new("event.type", "model_call"),
                    KeyValue::new("span.id", span_id.clone()),
                ];
                if let Some(dur) = duration_ms {
                    attrs.push(KeyValue::new("duration_ms", dur as i64));
                }
                if let Some(tok) = request_tokens {
                    attrs.push(KeyValue::new("tokens.in", tok as i64));
                }
                if let Some(tok) = response_tokens {
                    attrs.push(KeyValue::new("tokens.out", tok as i64));
                }
                if let Some(ref error) = error {
                    attrs.push(KeyValue::new("error", error.clone()));
                }
                (format!("model:{}", model), attrs)
            }
            TelemetryEvent::ToolExecution {
                tool_name,
                duration_ms,
                result,
                ..
            } => {
                let mut attrs = vec![
                    KeyValue::new("service.name", "allthecodes"),
                    KeyValue::new("event.type", "tool_execution"),
                ];
                if let Some(dur) = duration_ms {
                    attrs.push(KeyValue::new("duration_ms", dur as i64));
                }
                attrs.push(KeyValue::new("tool.result", result.clone()));
                (format!("tool:{}", tool_name), attrs)
            }
            TelemetryEvent::HookExecution {
                hook_name,
                duration_ms,
                result,
            } => {
                let mut attrs = vec![
                    KeyValue::new("service.name", "allthecodes"),
                    KeyValue::new("event.type", "hook_execution"),
                ];
                if let Some(dur) = duration_ms {
                    attrs.push(KeyValue::new("duration_ms", dur as i64));
                }
                attrs.push(KeyValue::new("hook.result", result));
                (format!("hook:{}", hook_name), attrs)
            }
            TelemetryEvent::InputEvent {
                input_summary,
                input_type,
            } => {
                let attrs = vec![
                    KeyValue::new("service.name", "allthecodes"),
                    KeyValue::new("event.type", "input_event"),
                    KeyValue::new("input.type", input_type.clone()),
                    KeyValue::new("input.summary", input_summary),
                ];
                (format!("input:{}", input_type), attrs)
            }
        };

        let span_builder = tracer.span_builder(span_name).with_attributes(attrs);
        tracer.build_with_context(span_builder, &opentelemetry::Context::new());
    }

    tracing::debug!(count = count, "exported telemetry events to langfuse");
}

fn env_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_parse<T>(key: &str) -> Option<T>
where
    T: std::str::FromStr,
{
    env_var(key)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn langfuse_enabled_requires_both_keys() {
        std::env::remove_var("LANGFUSE_PUBLIC_KEY");
        std::env::remove_var("LANGFUSE_SECRET_KEY");
        assert!(!is_langfuse_enabled());

        std::env::set_var("LANGFUSE_PUBLIC_KEY", "pk");
        assert!(!is_langfuse_enabled());

        std::env::set_var("LANGFUSE_SECRET_KEY", "sk");
        assert!(is_langfuse_enabled());

        std::env::remove_var("LANGFUSE_PUBLIC_KEY");
        std::env::remove_var("LANGFUSE_SECRET_KEY");
    }
}
