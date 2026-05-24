//! Local recurring-task scheduler — shared infrastructure for `/loop`
//! (issue #58) and `/schedule` (issue #60).
//!
//! Tasks are persisted to `{data_root}/scheduled_tasks.json` and guarded by
//! a sibling lockfile so concurrent sessions don't step on each other. The
//! scheduler itself is intentionally passive: it exposes CRUD + `due_tasks`
//! polling. The daemon (or whatever orchestration layer runs on top) is
//! responsible for actually firing a task when it reports due.
//!
//! The split between `/loop` and `/schedule`:
//!
//! - `/loop` — user-friendly wrapper. Parses a human interval (`5m`, `1h`)
//!   into a schedule, creates a recurring task, and reports the task back to
//!   the caller so the host can also execute it once immediately.
//! - `/schedule` — raw management surface over the same store: list / add /
//!   remove / inspect / trigger.
//!
//! The remote-triggers capability (GitHub/remote agent cron) is explicitly
//! *not* covered here. Issue #60's first milestone is local cron, and the
//! two capability lines stay separate — see `SchedulerKind`.

pub mod interval;
pub mod store;
pub mod task;

pub use interval::{parse_interval, Interval};
pub use store::{SchedulerError, SchedulerStore};
pub use task::{ScheduledTask, SchedulerKind, TaskId, TaskPayload};
