//! 累计耗时追踪 — 线程安全，无需持锁
//!
//! 用于全局级别的 API / Tool 耗时统计。

use std::sync::atomic::{AtomicU64, Ordering};

/// 无锁累计耗时追踪器。
///
/// 使用 AtomicU64 实现，多线程环境下无需持锁。
/// 适用于只增不减的全局统计场景。
pub struct DurationTracker {
    total_ms: AtomicU64,
    count: AtomicU64,
}

impl DurationTracker {
    pub const fn new() -> Self {
        Self {
            total_ms: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// 记录一次操作的耗时。
    pub fn record(&self, duration_ms: u64) {
        self.total_ms.fetch_add(duration_ms, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// 总累计耗时 (ms)。
    pub fn total_ms(&self) -> u64 {
        self.total_ms.load(Ordering::Relaxed)
    }

    /// 操作次数。
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// 平均耗时 (ms)，无操作时返回 0。
    pub fn avg_ms(&self) -> u64 {
        let c = self.count();
        if c == 0 {
            0
        } else {
            self.total_ms() / c
        }
    }
}

impl Default for DurationTracker {
    fn default() -> Self {
        Self::new()
    }
}

// DurationTracker 使用 Atomic，天然 Send + Sync，
// 但因为包含 AtomicU64 字段，编译器已自动推导。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let t = DurationTracker::new();
        assert_eq!(t.total_ms(), 0);
        assert_eq!(t.count(), 0);
        assert_eq!(t.avg_ms(), 0);
    }

    #[test]
    fn record_accumulates() {
        let t = DurationTracker::new();
        t.record(100);
        t.record(200);
        t.record(300);
        assert_eq!(t.total_ms(), 600);
        assert_eq!(t.count(), 3);
        assert_eq!(t.avg_ms(), 200);
    }

    #[test]
    fn concurrent_records() {
        use std::sync::Arc;
        use std::thread;

        let t = Arc::new(DurationTracker::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let tracker = Arc::clone(&t);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    tracker.record(1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(t.count(), 1000);
        assert_eq!(t.total_ms(), 1000);
    }
}
