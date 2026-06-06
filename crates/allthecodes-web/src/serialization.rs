//! Request serialization and session ownership coordination.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use allthecodes_protocol::SerializationScope;
use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOwner {
    #[default]
    None,
    ChatStream,
    TuiPty,
    IpcWs,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionOwnership {
    pub owner: SessionOwner,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct SerializationLayer {
    queues: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    owner: Arc<RwLock<SessionOwnership>>,
}

impl SerializationLayer {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn run_scoped<K, F, Fut, R>(&self, scope: &SerializationScope, key: K, f: F) -> R
    where
        K: Into<String>,
        F: FnOnce() -> Fut,
        Fut: Future<Output = R>,
    {
        let fallback_key = key.into();
        let Some(queue_key) = self.queue_key(scope, fallback_key) else {
            return f().await;
        };

        let semaphore = {
            let mut queues = self.queues.lock().await;
            queues
                .entry(queue_key)
                .or_insert_with(|| Arc::new(Semaphore::new(1)))
                .clone()
        };

        let Ok(_permit) = semaphore.acquire_owned().await else {
            return f().await;
        };

        f().await
    }

    pub fn ownership_snapshot(&self) -> SessionOwnership {
        self.owner.read().clone()
    }

    pub fn try_claim_owner(
        &self,
        owner: SessionOwner,
        session_id: String,
    ) -> Result<(), SessionOwnership> {
        let mut current = self.owner.write();
        if current.owner != SessionOwner::None {
            return Err(current.clone());
        }
        *current = SessionOwnership {
            owner,
            session_id: Some(session_id),
        };
        Ok(())
    }

    pub fn release_owner(&self, owner: SessionOwner) {
        let mut current = self.owner.write();
        if current.owner == owner {
            *current = SessionOwnership::default();
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
