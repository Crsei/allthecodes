//! Durable session record/replay primitives.
//!
//! This module owns the append-only rollout log format. Phase 1 keeps it
//! isolated from engine submit paths; later phases will make engine resume and
//! shutdown use these APIs.

pub mod config;
pub mod index;
pub mod migration;
pub mod paths;
pub mod policy;
pub mod reader;
pub mod reconstruct;
pub mod recorder;
pub mod redaction;
pub mod types;

#[cfg(test)]
pub(crate) mod fixtures;

pub use config::RecordReplayConfig;
pub use index::{
    append_record_items, ensure_rollout_for_session, lookup_rollout, reindex_rollouts,
    upsert_rollout, SessionRolloutIndexEntry, SESSION_ROLLOUTS_SCHEMA_SQL,
};
pub use migration::{create_rollout_from_messages, migrate_legacy_session};
pub use policy::{classify_record_item, may_drop_under_pressure, should_persist, RecordClass};
pub use reader::{read_rollout_file, ReplayReadResult, ReplayReadWarning, ReplayReader};
pub use reconstruct::{
    message_seq_for_uuid, reconstruct_messages, reconstruct_recorded_messages,
    recorded_message_uuid, recorded_messages_to_typed, PendingInteraction,
    ReconstructedRecordMessage, ReconstructedRecordSession,
};
pub use recorder::{
    RecorderCommand, RecorderOpenMode, RecorderStats, SessionRecorderHandle,
    DEFAULT_RECORDER_CHANNEL_CAPACITY,
};
pub use types::{RecordItem, RecordLine, RECORD_SCHEMA_VERSION};
