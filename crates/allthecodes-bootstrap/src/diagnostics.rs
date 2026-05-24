//! 诊断信息收集 — 内存错误日志 + 慢操作追踪
//!
//! 对应 TypeScript: bootstrap/state.ts 中的
//! `inMemoryErrorLog` 和 `slowOperations` 字段。
//!
//! 不写磁盘，仅在进程生命周期内保留。

use std::collections::VecDeque;
use std::time::Instant;

// ---------------------------------------------------------------------------
// ErrorLog
// ---------------------------------------------------------------------------

/// 内存错误日志条目。
#[derive(Debug, Clone)]
pub struct ErrorEntry {
    pub message: String,
    pub context: Option<String>,
    pub timestamp: Instant,
}

/// 固定容量的内存错误日志。
///
/// 超过容量时自动丢弃最早的条目 (ring buffer 语义)。
pub struct ErrorLog {
    entries: VecDeque<ErrorEntry>,
    max_entries: usize,
}

impl ErrorLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(256)),
            max_entries,
        }
    }

    /// 记录一条错误。
    pub fn push(&mut self, message: impl Into<String>, context: Option<String>) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(ErrorEntry {
            message: message.into(),
            context,
            timestamp: Instant::now(),
        });
    }

    /// 获取所有条目。
    pub fn entries(&self) -> &VecDeque<ErrorEntry> {
        &self.entries
    }

    /// 条目数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空日志。
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ErrorLog {
    fn default() -> Self {
        Self::new(100)
    }
}

// ---------------------------------------------------------------------------
// SlowOperationTracker
// ---------------------------------------------------------------------------

/// 慢操作记录条目。
#[derive(Debug, Clone)]
pub struct SlowOperation {
    pub name: String,
    pub duration_ms: u64,
    pub timestamp: Instant,
}

/// 慢操作追踪器 — 仅记录超过阈值的操作。
pub struct SlowOperationTracker {
    operations: VecDeque<SlowOperation>,
    threshold_ms: u64,
    max_entries: usize,
}

impl SlowOperationTracker {
    pub fn new(threshold_ms: u64, max_entries: usize) -> Self {
        Self {
            operations: VecDeque::with_capacity(max_entries.min(256)),
            threshold_ms,
            max_entries,
        }
    }

    /// 记录一次操作。仅当 `duration_ms >= threshold_ms` 时保留。
    pub fn record(&mut self, name: impl Into<String>, duration_ms: u64) {
        if duration_ms < self.threshold_ms {
            return;
        }
        if self.operations.len() >= self.max_entries {
            self.operations.pop_front();
        }
        self.operations.push_back(SlowOperation {
            name: name.into(),
            duration_ms,
            timestamp: Instant::now(),
        });
    }

    /// 获取所有记录的慢操作。
    pub fn operations(&self) -> &VecDeque<SlowOperation> {
        &self.operations
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl Default for SlowOperationTracker {
    fn default() -> Self {
        Self::new(500, 50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- ErrorLog tests --

    #[test]
    fn error_log_push_and_read() {
        let mut log = ErrorLog::new(5);
        log.push("err1", None);
        log.push("err2", Some("ctx".into()));
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].message, "err1");
        assert_eq!(log.entries()[1].context.as_deref(), Some("ctx"));
    }

    #[test]
    fn error_log_evicts_oldest() {
        let mut log = ErrorLog::new(3);
        log.push("a", None);
        log.push("b", None);
        log.push("c", None);
        log.push("d", None); // evicts "a"
        assert_eq!(log.len(), 3);
        assert_eq!(log.entries()[0].message, "b");
        assert_eq!(log.entries()[2].message, "d");
    }

    #[test]
    fn error_log_clear() {
        let mut log = ErrorLog::new(10);
        log.push("x", None);
        log.clear();
        assert!(log.is_empty());
    }

    // -- SlowOperationTracker tests --

    #[test]
    fn slow_op_below_threshold_ignored() {
        let mut tracker = SlowOperationTracker::new(500, 10);
        tracker.record("fast_op", 100);
        assert!(tracker.is_empty());
    }

    #[test]
    fn slow_op_at_threshold_recorded() {
        let mut tracker = SlowOperationTracker::new(500, 10);
        tracker.record("exactly_500", 500);
        assert_eq!(tracker.len(), 1);
        assert_eq!(tracker.operations()[0].name, "exactly_500");
    }

    #[test]
    fn slow_op_evicts_oldest() {
        let mut tracker = SlowOperationTracker::new(0, 2);
        tracker.record("a", 10);
        tracker.record("b", 20);
        tracker.record("c", 30); // evicts "a"
        assert_eq!(tracker.len(), 2);
        assert_eq!(tracker.operations()[0].name, "b");
    }
}
