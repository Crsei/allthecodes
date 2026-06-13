//! Request serialization and session ownership coordination.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use allthecodes_protocol::{AccessMode, SerializationScope};
use tokio::sync::{Mutex, RwLock as TokioRwLock};
use tokio::time::{timeout, Duration};

#[derive(Clone)]
pub struct SerializationLayer {
    queues: Arc<Mutex<HashMap<String, Arc<TokioRwLock<()>>>>>,
}

impl Default for SerializationLayer {
    fn default() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SerializationLayer {
    pub fn new() -> Self {
        Self::default()
    }

    const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

    pub async fn run_scoped<K, F, Fut, R>(&self, scope: &SerializationScope, key: K, f: F) -> R
    where
        K: Into<String>,
        F: FnOnce() -> Fut,
        Fut: Future<Output = R>,
    {
        let fallback_key = key.into();
        let Some(queue_key) = self.queue_key(scope, fallback_key.clone()) else {
            return f().await;
        };

        let lock = {
            let mut queues = self.queues.lock().await;
            queues
                .entry(queue_key)
                .or_insert_with(|| Arc::new(TokioRwLock::new(())))
                .clone()
        };

        let access_mode = scope.access_mode();

        match access_mode {
            AccessMode::Exclusive => {
                let _guard = match timeout(Self::LOCK_TIMEOUT, lock.write()).await {
                    Ok(guard) => guard,
                    Err(_) => {
                        tracing::error!(
                            "SerializationLayer: write lock timeout on key '{}'",
                            fallback_key
                        );
                        return f().await;
                    }
                };
                f().await
            }
            AccessMode::SharedRead => {
                let _guard = match timeout(Self::LOCK_TIMEOUT, lock.read()).await {
                    Ok(guard) => guard,
                    Err(_) => {
                        tracing::error!(
                            "SerializationLayer: read lock timeout on key '{}'",
                            fallback_key
                        );
                        return f().await;
                    }
                };
                f().await
            }
        }
    }

    fn queue_key(&self, scope: &SerializationScope, fallback_key: String) -> Option<String> {
        match scope {
            SerializationScope::Concurrent => None,
            SerializationScope::PerProcess => Some("process".to_string()),
            SerializationScope::PerConnection => Some(format!("connection:{fallback_key}")),
            SerializationScope::PerKey { field, key } => {
                let key = if key.is_empty() { &fallback_key } else { key };
                Some(format!("{field}:{key}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn per_key_scope_serializes_same_key() {
        let layer = SerializationLayer::new();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let scope = SerializationScope::PerKey {
            field: "session",
            key: "s1".to_string(),
        };

        let first = tokio::spawn(run_probe(
            layer.clone(),
            scope.clone(),
            active.clone(),
            peak.clone(),
        ));
        let second = tokio::spawn(run_probe(layer, scope, active, peak.clone()));

        first.await.expect("first task should finish");
        second.await.expect("second task should finish");

        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    async fn run_probe(
        layer: SerializationLayer,
        scope: SerializationScope,
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    ) {
        layer
            .run_scoped(&scope, "fallback", || async {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            })
            .await;
    }
}
