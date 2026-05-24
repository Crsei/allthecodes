//! Generic client-side request registry for UI/runtime request-response flows.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PendingRequest<T> {
    pub id: String,
    pub payload: T,
    created_at: Instant,
}

impl<T> PendingRequest<T> {
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

#[derive(Debug, Clone)]
pub struct AppServerRequests<T> {
    next_id: u64,
    requests: BTreeMap<String, PendingRequest<T>>,
}

impl<T> Default for AppServerRequests<T> {
    fn default() -> Self {
        Self {
            next_id: 1,
            requests: BTreeMap::new(),
        }
    }
}

impl<T> AppServerRequests<T> {
    pub fn insert(&mut self, payload: T) -> String {
        let id = format!("ui-{}", self.next_id);
        self.next_id += 1;
        self.requests.insert(
            id.clone(),
            PendingRequest {
                id: id.clone(),
                payload,
                created_at: Instant::now(),
            },
        );
        id
    }

    pub fn take(&mut self, id: &str) -> Option<T> {
        self.requests.remove(id).map(|request| request.payload)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.requests.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.requests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    pub fn expire_older_than(&mut self, max_age: Duration) -> Vec<PendingRequest<T>> {
        let expired_ids = self
            .requests
            .iter()
            .filter_map(|(id, request)| (request.age() > max_age).then_some(id.clone()))
            .collect::<Vec<_>>();
        expired_ids
            .into_iter()
            .filter_map(|id| self.requests.remove(&id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_and_takes_requests() {
        let mut requests = AppServerRequests::default();
        let id = requests.insert("payload");
        assert!(requests.contains(&id));
        assert_eq!(requests.take(&id), Some("payload"));
    }

    #[test]
    fn exposes_size_and_request_metadata() {
        let mut requests = AppServerRequests::default();
        assert!(requests.is_empty());

        let id = requests.insert("payload");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests.requests[&id].id, id);
        assert!(requests.requests[&id].age() < Duration::from_secs(1));
    }

    #[test]
    fn expires_older_requests() {
        let mut requests = AppServerRequests::default();
        let id = requests.insert("payload");

        let expired = requests.expire_older_than(Duration::ZERO);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, id);
        assert!(requests.is_empty());
    }
}
