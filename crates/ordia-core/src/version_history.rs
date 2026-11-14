//! Point-in-time document version history snapshot store with CRC32 integrity and retention pruning (`M1-07`, `PLAN.md` §4.6).

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::document::atomic_write_bytes;
use crate::journal::JournalError;

/// Retention limits governing automatic cleanup of historical document snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SnapshotRetentionPolicy {
    /// Maximum number of snapshots kept per document (`>= 1`).
    pub max_snapshots: usize,
    /// Maximum cumulative byte budget kept per document (`>= 1`).
    pub max_total_bytes: u64,
}

impl SnapshotRetentionPolicy {
    /// Creates an explicit [`SnapshotRetentionPolicy`].
    #[must_use]
    pub const fn new(max_snapshots: usize, max_total_bytes: u64) -> Self {
        Self {
            max_snapshots: if max_snapshots == 0 { 1 } else { max_snapshots },
            max_total_bytes: if max_total_bytes == 0 {
                1024
            } else {
                max_total_bytes
            },
        }
    }
}

impl Default for SnapshotRetentionPolicy {
    fn default() -> Self {
        Self {
            max_snapshots: 20,
            max_total_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Metadata entry for a single historical snapshot stored under `<data_dir>/history/<doc_key>/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VersionSnapshotMeta {
    /// Monotonically increasing 1-based version identifier for this document.
    pub version_id: u64,
    /// UTC creation timestamp in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Optional user-facing or automatic save label.
    pub label: String,
    /// Size of the stored snapshot blob in bytes.
    pub byte_len: u64,
    /// CRC32 checksum of the stored snapshot bytes.
    pub crc32: u32,
    /// Relative blob filename inside the document's history directory.
    pub blob_file: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct HistoryManifest {
    next_version_id: u64,
    snapshots: Vec<VersionSnapshotMeta>,
}

/// Manages local point-in-time version history snapshots under `<data_dir>/history/`.
#[derive(Clone, Debug)]
pub struct VersionHistoryStore {
    data_dir: PathBuf,
    policy: SnapshotRetentionPolicy,
}

impl VersionHistoryStore {
    /// Creates a [`VersionHistoryStore`] rooted at `data_dir` with default retention limits.
    #[must_use]
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            policy: SnapshotRetentionPolicy::default(),
        }
    }

    /// Overrides the [`SnapshotRetentionPolicy`].
    #[must_use]
    pub const fn with_policy(mut self, policy: SnapshotRetentionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Derives a filesystem-safe document key from a canonical path or document title.
    #[must_use]
    pub fn doc_key_for_path(path: &Path) -> String {
        let raw = path.to_string_lossy();
        let hash = crc32fast::hash(raw.as_bytes());
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("doc")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(24)
            .collect::<String>();
        let clean_stem = if stem.is_empty() { "doc" } else { &stem };
        format!("{clean_stem}-{hash:08x}")
    }

    fn doc_dir(&self, doc_key: &str) -> PathBuf {
        self.data_dir.join("history").join(doc_key)
    }

    fn load_manifest(&self, doc_key: &str) -> Result<HistoryManifest, JournalError> {
        let manifest_path = self.doc_dir(doc_key).join("manifest.toml");
        if !manifest_path.exists() {
            return Ok(HistoryManifest {
                next_version_id: 1,
                snapshots: Vec::new(),
            });
        }
        let raw = fs::read_to_string(&manifest_path).map_err(|source| JournalError::Io {
            path: manifest_path.clone(),
            source,
        })?;
        toml::from_str(&raw).map_err(|e| JournalError::Serialization(e.to_string()))
    }

    fn save_manifest(&self, doc_key: &str, manifest: &HistoryManifest) -> Result<(), JournalError> {
        let manifest_path = self.doc_dir(doc_key).join("manifest.toml");
        let encoded = toml::to_string_pretty(manifest)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;
        atomic_write_bytes(&manifest_path, encoded.as_bytes(), |_| Ok(()))
            .map_err(|e| JournalError::Serialization(e.to_string()))
    }
}

impl VersionHistoryStore {
    /// Atomically records a new snapshot blob for `doc_key` and prunes expired versions per [`SnapshotRetentionPolicy`].
    pub fn record_snapshot(
        &self,
        doc_key: &str,
        label: impl Into<String>,
        bytes: &[u8],
    ) -> Result<VersionSnapshotMeta, JournalError> {
        let dir = self.doc_dir(doc_key);
        fs::create_dir_all(&dir).map_err(|source| JournalError::Io {
            path: dir.clone(),
            source,
        })?;

        let mut manifest = self.load_manifest(doc_key)?;
        let vid = manifest.next_version_id.max(1);
        manifest.next_version_id = vid + 1;

        let blob_file = format!("v{vid:06}.snap");
        let blob_path = dir.join(&blob_file);
        let crc = crc32fast::hash(bytes);
        atomic_write_bytes(&blob_path, bytes, |_| Ok(()))
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        let meta = VersionSnapshotMeta {
            version_id: vid,
            created_at_unix_ms: Utc::now().timestamp_millis(),
            label: label.into(),
            byte_len: bytes.len() as u64,
            crc32: crc,
            blob_file,
        };
        manifest.snapshots.push(meta.clone());
        self.prune_manifest(&dir, &mut manifest);
        self.save_manifest(doc_key, &manifest)?;
        Ok(meta)
    }

    /// Lists retained snapshots for `doc_key` ordered from oldest to newest.
    pub fn list_snapshots(&self, doc_key: &str) -> Result<Vec<VersionSnapshotMeta>, JournalError> {
        Ok(self.load_manifest(doc_key)?.snapshots)
    }

    /// Loads and CRC32-validates snapshot `version_id` for `doc_key`.
    pub fn load_snapshot(&self, doc_key: &str, version_id: u64) -> Result<Vec<u8>, JournalError> {
        let manifest = self.load_manifest(doc_key)?;
        let entry = manifest
            .snapshots
            .iter()
            .find(|s| s.version_id == version_id)
            .ok_or_else(|| JournalError::NotFound(format!("{doc_key}@{version_id}")))?;
        let blob_path = self.doc_dir(doc_key).join(&entry.blob_file);
        let bytes = fs::read(&blob_path).map_err(|source| JournalError::Io {
            path: blob_path,
            source,
        })?;
        let actual_crc = crc32fast::hash(&bytes);
        if actual_crc != entry.crc32 {
            return Err(JournalError::Serialization(format!(
                "snapshot CRC32 mismatch for {doc_key}@{version_id}: expected {:08x}, got {actual_crc:08x}",
                entry.crc32
            )));
        }
        Ok(bytes)
    }

    fn prune_manifest(&self, dir: &Path, manifest: &mut HistoryManifest) {
        while manifest.snapshots.len() > self.policy.max_snapshots {
            let removed = manifest.snapshots.remove(0);
            let _ = fs::remove_file(dir.join(&removed.blob_file));
        }
        while manifest.snapshots.len() > 1 {
            let total: u64 = manifest.snapshots.iter().map(|s| s.byte_len).sum();
            if total <= self.policy.max_total_bytes {
                break;
            }
            let removed = manifest.snapshots.remove(0);
            let _ = fs::remove_file(dir.join(&removed.blob_file));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_history_records_verifies_crc_and_prunes_oldest_snapshots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = VersionHistoryStore::with_data_dir(dir.path())
            .with_policy(SnapshotRetentionPolicy::new(2, 10_000));
        let key = VersionHistoryStore::doc_key_for_path(Path::new("/tmp/Budget_2026.xlsx"));

        let v1 = store
            .record_snapshot(&key, "Save 1", b"state-1")
            .expect("v1");
        let v2 = store
            .record_snapshot(&key, "Save 2", b"state-2")
            .expect("v2");
        let v3 = store
            .record_snapshot(&key, "Save 3", b"state-3")
            .expect("v3");
        assert_eq!(v1.version_id, 1);
        assert_eq!(v2.version_id, 2);
        assert_eq!(v3.version_id, 3);

        let kept = store.list_snapshots(&key).expect("list");
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].version_id, 2);
        assert_eq!(kept[1].version_id, 3);
        assert_eq!(store.load_snapshot(&key, 3).expect("load v3"), b"state-3");
    }
}
