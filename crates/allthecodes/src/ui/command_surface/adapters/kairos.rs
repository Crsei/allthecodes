use std::sync::{OnceLock, RwLock};

use allthecodes_types::kairos::KairosRuntimeSnapshot;

#[derive(Debug, Clone, Default)]
pub(crate) struct KairosSurfaceSnapshot {
    pub(crate) runtime: Option<KairosRuntimeSnapshot>,
    pub(crate) error: Option<String>,
}

static SNAPSHOT: OnceLock<RwLock<KairosSurfaceSnapshot>> = OnceLock::new();

pub(crate) fn latest_kairos_snapshot() -> KairosSurfaceSnapshot {
    SNAPSHOT
        .get_or_init(|| RwLock::new(KairosSurfaceSnapshot::default()))
        .read()
        .map(|snapshot| snapshot.clone())
        .unwrap_or_default()
}

pub(crate) fn set_kairos_snapshot(snapshot: Result<KairosRuntimeSnapshot, String>) {
    let value = match snapshot {
        Ok(runtime) => KairosSurfaceSnapshot {
            runtime: Some(runtime),
            error: None,
        },
        Err(error) => KairosSurfaceSnapshot {
            runtime: None,
            error: Some(error),
        },
    };
    if let Ok(mut target) = SNAPSHOT
        .get_or_init(|| RwLock::new(KairosSurfaceSnapshot::default()))
        .write()
    {
        *target = value;
    }
}
