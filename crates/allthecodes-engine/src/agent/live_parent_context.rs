//! Read-only-on-demand parent context channels for live fork agents.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use allthecodes_types::agent_types::LiveParentContextPaths;
use anyhow::{Context, Result};
use serde_json::json;

#[derive(Debug, Clone)]
struct ActiveChannel {
    parent_session_id: String,
    paths: LiveParentContextPaths,
    next_seq: u64,
}

static ACTIVE_CHANNELS: LazyLock<Mutex<HashMap<String, ActiveChannel>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn create_channel(
    parent_session_id: &str,
    fork_agent_id: &str,
    initial_snapshot: &str,
) -> Result<LiveParentContextPaths> {
    // TODO(upstream fork parity): enforce OS/sandbox-level child read-only
    // access. The current contract is prompt/policy read-only so the parent
    // publisher can continue appending to the same files.
    let directory = channel_root(parent_session_id).join(sanitize_segment(fork_agent_id));
    fs::create_dir_all(&directory)
        .with_context(|| format!("create live parent channel {}", directory.display()))?;

    let snapshot = directory.join("parent-context.md");
    let updates = directory.join("parent-updates.ndjson");
    let latest_diff = directory.join("parent-context.diff");
    fs::write(&snapshot, initial_snapshot)
        .with_context(|| format!("write parent context snapshot {}", snapshot.display()))?;
    fs::write(&latest_diff, "")
        .with_context(|| format!("write parent context diff {}", latest_diff.display()))?;
    let initial_update = json!({
        "seq": 1,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "source": "snapshot",
        "text": "Initial parent context snapshot written"
    });
    fs::write(&updates, format!("{}\n", initial_update))
        .with_context(|| format!("write parent context updates {}", updates.display()))?;

    let paths = LiveParentContextPaths {
        directory: directory.display().to_string(),
        snapshot: snapshot.display().to_string(),
        updates: updates.display().to_string(),
        latest_diff: Some(latest_diff.display().to_string()),
        latest_seq: 1,
    };
    ACTIVE_CHANNELS.lock().expect("live channel lock").insert(
        fork_agent_id.to_string(),
        ActiveChannel {
            parent_session_id: parent_session_id.to_string(),
            paths: paths.clone(),
            next_seq: 2,
        },
    );
    Ok(paths)
}

pub(crate) fn publish_parent_update(parent_session_id: &str, source: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let mut channels = ACTIVE_CHANNELS.lock().expect("live channel lock");
    for channel in channels
        .values_mut()
        .filter(|channel| channel.parent_session_id == parent_session_id)
    {
        if let Err(error) = append_update(channel, source, text) {
            tracing::warn!(%error, "failed to append live parent context update");
        }
    }
}

pub(super) fn close_channel(fork_agent_id: &str, status: &str) {
    let Some(mut channel) = ACTIVE_CHANNELS
        .lock()
        .expect("live channel lock")
        .remove(fork_agent_id)
    else {
        return;
    };
    let _ = append_update(
        &mut channel,
        "lifecycle",
        &format!("Live parent channel closed: {status}"),
    );
    // Files intentionally remain under the session run directory for resume,
    // crash recovery, and post-run inspection. Only the in-process publisher
    // registration is removed. A future retention policy may garbage-collect
    // old session run directories.
}

fn append_update(channel: &mut ActiveChannel, source: &str, text: &str) -> Result<()> {
    let seq = channel.next_seq;
    channel.next_seq += 1;
    let event = json!({
        "seq": seq,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "source": source,
        "text": text
    });
    let mut file = OpenOptions::new()
        .append(true)
        .open(&channel.paths.updates)
        .with_context(|| format!("open {}", channel.paths.updates))?;
    writeln!(file, "{event}")?;

    let diff = format!("## update {seq} · {source}\n\n{text}\n");
    if let Some(path) = channel.paths.latest_diff.as_deref() {
        fs::write(path, &diff).with_context(|| format!("write {path}"))?;
    }
    let mut snapshot = OpenOptions::new()
        .append(true)
        .open(&channel.paths.snapshot)
        .with_context(|| format!("open {}", channel.paths.snapshot))?;
    writeln!(snapshot, "\n{diff}")?;
    channel.paths.latest_seq = seq;
    Ok(())
}

fn channel_root(parent_session_id: &str) -> PathBuf {
    allthecodes_config::paths::runs_dir(&sanitize_segment(parent_session_id)).join("fork-context")
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard(Option<std::ffi::OsString>);

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.0.take() {
                std::env::set_var("ALLTHECODES_HOME", value);
            } else {
                std::env::remove_var("ALLTHECODES_HOME");
            }
        }
    }

    #[test]
    #[serial]
    fn creates_and_updates_live_channel_files() {
        let temp = tempfile::tempdir().unwrap();
        let guard = EnvGuard(std::env::var_os("ALLTHECODES_HOME"));
        std::env::set_var("ALLTHECODES_HOME", temp.path());
        let paths = create_channel("parent", "fork-1", "initial").unwrap();
        publish_parent_update("parent", "assistant", "new output");
        close_channel("fork-1", "completed");

        assert!(std::path::Path::new(&paths.snapshot).is_file());
        assert!(fs::read_to_string(&paths.snapshot)
            .unwrap()
            .contains("new output"));
        let updates = fs::read_to_string(&paths.updates).unwrap();
        assert!(updates.contains("\"seq\":2"));
        assert!(updates.contains("channel closed"));
        drop(guard);
    }
}
