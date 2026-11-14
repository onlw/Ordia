//! Append-only CRC32-framed operation journal and startup crash-recovery manager (`M1-07`, `PLAN.md` §4.6).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use crc32fast::Hasher;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

/// 8-byte binary header identifying an Ordia v1 operation journal (`"ORDJNL01"`).
const JOURNAL_MAGIC: &[u8; 8] = b"ORDJNL01";
/// Fixed per-record header width: `seq (8) + timestamp_ms (8) + payload_len (4) + crc32 (4) = 24` bytes.
const FRAME_HEADER_LEN: usize = 24;
/// Defensive upper bound (`64 MiB`) on a single journal frame payload to reject corrupt length prefixes.
const MAX_FRAME_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Errors emitted by the operation journal and version-history subsystems.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JournalError {
    /// Filesystem I/O failure.
    #[error("journal I/O error at `{}`: {source}", path.display())]
    Io {
        /// Affected path.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// Serialization or metadata encoding error.
    #[error("journal serialization error: {0}")]
    Serialization(String),
    /// Invalid or unrecognized journal file magic header.
    #[error("invalid journal header in `{}`", path.display())]
    InvalidHeader {
        /// Journal file path.
        path: PathBuf,
    },
    /// Requested snapshot or session was not found.
    #[error("not found: `{0}`")]
    NotFound(String),
}

/// Configurable policy controlling when buffered operations are flushed (`sync_data`) to disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct JournalFlushPolicy {
    /// Automatically flush after accumulating this many appended operations (`>= 1`).
    pub flush_every_n_ops: usize,
    /// Automatically flush when at least this much wall-clock time has elapsed since the previous flush.
    pub flush_interval: Duration,
}

impl JournalFlushPolicy {
    /// Constructs an explicit [`JournalFlushPolicy`].
    #[must_use]
    pub const fn new(flush_every_n_ops: usize, flush_interval: Duration) -> Self {
        Self {
            flush_every_n_ops: if flush_every_n_ops == 0 {
                1
            } else {
                flush_every_n_ops
            },
            flush_interval,
        }
    }
}

impl Default for JournalFlushPolicy {
    fn default() -> Self {
        Self {
            flush_every_n_ops: 10,
            flush_interval: Duration::from_secs(30),
        }
    }
}

/// Persisted metadata describing an editing session (`meta.toml`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct JournalSessionMeta {
    /// Unique session identifier (`<doc>-<pid>-<seq>`).
    pub session_id: String,
    /// Human-readable document title (e.g. `"Budget_2026.xlsx"`).
    pub document_title: String,
    /// Optional backing file path on disk.
    pub source_path: Option<PathBuf>,
    /// UTC creation timestamp in Unix milliseconds.
    pub created_at_unix_ms: i64,
}

impl JournalSessionMeta {
    /// Creates metadata for a new journal session.
    #[must_use]
    pub fn new(
        session_id: impl Into<String>,
        document_title: impl Into<String>,
        source_path: Option<PathBuf>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            document_title: document_title.into(),
            source_path,
            created_at_unix_ms: Utc::now().timestamp_millis(),
        }
    }
}

/// Append-only binary operation journal writer generic over `Op`.
///
/// Operations appended via [`Self::append`] are CRC32-framed into an in-memory buffer and flushed
/// (`write_all` + `sync_data`) according to [`JournalFlushPolicy`] or explicit [`Self::flush`].
/// Calling [`Self::close_cleanly`] removes the active session lock and temporary journal; if the
/// process crashes or `JournalWriter` is dropped without `close_cleanly`, `active.lock` remains so
/// [`JournalRecoveryManager`] detects and recovers up to the last flushed operation on next startup.
pub struct JournalWriter<Op> {
    session_dir: PathBuf,
    journal_path: PathBuf,
    lock_path: PathBuf,
    file: File,
    policy: JournalFlushPolicy,
    next_seq: u64,
    flushed_seq: u64,
    unflushed_ops: usize,
    pending_buf: Vec<u8>,
    last_flush: Instant,
    _marker: PhantomData<fn(&Op)>,
}

impl<Op: Serialize> JournalWriter<Op> {
    /// Opens a new session journal under `<data_dir>/journals/<meta.session_id>/`.
    pub fn open(
        data_dir: &Path,
        meta: &JournalSessionMeta,
        policy: JournalFlushPolicy,
    ) -> Result<Self, JournalError> {
        let session_dir = data_dir.join("journals").join(&meta.session_id);
        fs::create_dir_all(&session_dir).map_err(|source| JournalError::Io {
            path: session_dir.clone(),
            source,
        })?;

        let meta_path = session_dir.join("meta.toml");
        let meta_toml =
            toml::to_string_pretty(meta).map_err(|e| JournalError::Serialization(e.to_string()))?;
        fs::write(&meta_path, meta_toml).map_err(|source| JournalError::Io {
            path: meta_path,
            source,
        })?;

        let lock_path = session_dir.join("active.lock");
        fs::write(&lock_path, format!("pid={}\n", std::process::id())).map_err(|source| {
            JournalError::Io {
                path: lock_path.clone(),
                source,
            }
        })?;

        let journal_path = session_dir.join("ops.journal");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&journal_path)
            .map_err(|source| JournalError::Io {
                path: journal_path.clone(),
                source,
            })?;
        file.write_all(JOURNAL_MAGIC)
            .and_then(|()| file.sync_data())
            .map_err(|source| JournalError::Io {
                path: journal_path.clone(),
                source,
            })?;

        Ok(Self {
            session_dir,
            journal_path,
            lock_path,
            file,
            policy,
            next_seq: 1,
            flushed_seq: 0,
            unflushed_ops: 0,
            pending_buf: Vec::new(),
            last_flush: Instant::now(),
            _marker: PhantomData,
        })
    }
}

impl<Op: Serialize> JournalWriter<Op> {
    /// Appends `op` to the journal buffer, automatically flushing to disk when [`JournalFlushPolicy`] triggers.
    ///
    /// Returns the 1-based monotonic sequence number assigned to `op`.
    pub fn append(&mut self, op: &Op) -> Result<u64, JournalError> {
        let payload =
            postcard::to_stdvec(op).map_err(|e| JournalError::Serialization(e.to_string()))?;
        let payload_len = u32::try_from(payload.len()).map_err(|_| {
            JournalError::Serialization("operation payload exceeds u32::MAX".to_string())
        })?;
        let seq = self.next_seq;
        self.next_seq += 1;
        let timestamp_ms = Utc::now().timestamp_millis();

        let crc = compute_frame_crc32(seq, timestamp_ms, payload_len, &payload);
        self.pending_buf.reserve(FRAME_HEADER_LEN + payload.len());
        self.pending_buf.extend_from_slice(&seq.to_le_bytes());
        self.pending_buf
            .extend_from_slice(&timestamp_ms.to_le_bytes());
        self.pending_buf
            .extend_from_slice(&payload_len.to_le_bytes());
        self.pending_buf.extend_from_slice(&crc.to_le_bytes());
        self.pending_buf.extend_from_slice(&payload);
        self.unflushed_ops += 1;

        if self.unflushed_ops >= self.policy.flush_every_n_ops
            || self.last_flush.elapsed() >= self.policy.flush_interval
        {
            self.flush()?;
        }
        Ok(seq)
    }

    /// Flushes all pending operation frames to disk and calls `sync_data()`.
    pub fn flush(&mut self) -> Result<(), JournalError> {
        if self.pending_buf.is_empty() {
            return Ok(());
        }
        self.file
            .write_all(&self.pending_buf)
            .and_then(|()| self.file.sync_data())
            .map_err(|source| JournalError::Io {
                path: self.journal_path.clone(),
                source,
            })?;
        self.pending_buf.clear();
        self.unflushed_ops = 0;
        self.flushed_seq = self.next_seq.saturating_sub(1);
        self.last_flush = Instant::now();
        Ok(())
    }

    /// Sequence number of the most recently flushed operation (`0` if none flushed yet).
    #[must_use]
    pub const fn flushed_seq(&self) -> u64 {
        self.flushed_seq
    }

    /// On-disk path of the underlying `ops.journal` binary log.
    #[must_use]
    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    /// Marks the session as cleanly saved/closed, removing `active.lock` and cleaning up the session folder.
    pub fn close_cleanly(mut self) -> Result<(), JournalError> {
        self.flush()?;
        drop(self.file);
        let _ = fs::remove_file(&self.lock_path);
        fs::remove_dir_all(&self.session_dir).map_err(|source| JournalError::Io {
            path: self.session_dir,
            source,
        })
    }
}

/// Summary of an unclosed editing session detected during application startup.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct UncleanSessionInfo {
    /// Session metadata from `meta.toml`.
    pub meta: JournalSessionMeta,
    /// Path to `ops.journal` on disk.
    pub journal_path: PathBuf,
    /// Size of `ops.journal` in bytes.
    pub journal_bytes: u64,
}

/// Result of replaying a crash-recovery journal up to its last intact CRC32-verified frame.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecoveredJournal<Op> {
    /// Session metadata.
    pub meta: JournalSessionMeta,
    /// Ordered sequence of valid committed operations (`(seq, op)`).
    pub operations: Vec<(u64, Op)>,
    /// Number of torn or corrupted trailing bytes safely skipped at the end of `ops.journal`.
    pub skipped_tail_bytes: u64,
}

/// Scans `<data_dir>/journals/` on startup to detect abnormal terminations and replay intact frames.
#[derive(Clone, Debug)]
pub struct JournalRecoveryManager {
    data_dir: PathBuf,
}

impl JournalRecoveryManager {
    /// Creates a recovery manager rooted at `data_dir`.
    #[must_use]
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Scans for sessions whose `active.lock` is still present on disk (indicating an abnormal exit).
    pub fn scan_unclean_sessions(&self) -> Result<Vec<UncleanSessionInfo>, JournalError> {
        let root = self.data_dir.join("journals");
        if !root.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&root).map_err(|source| JournalError::Io {
            path: root.clone(),
            source,
        })?;
        let mut found = Vec::new();
        for entry_res in entries {
            let entry = entry_res.map_err(|source| JournalError::Io {
                path: root.clone(),
                source,
            })?;
            let dir = entry.path();
            if !dir.is_dir() || !dir.join("active.lock").exists() {
                continue;
            }
            let journal_path = dir.join("ops.journal");
            let meta_path = dir.join("meta.toml");
            if !journal_path.exists() || !meta_path.exists() {
                continue;
            }
            let raw_meta = match fs::read_to_string(&meta_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let Ok(meta) = toml::from_str::<JournalSessionMeta>(&raw_meta) else {
                continue;
            };
            let journal_bytes = fs::metadata(&journal_path).map(|m| m.len()).unwrap_or(0);
            found.push(UncleanSessionInfo {
                meta,
                journal_path,
                journal_bytes,
            });
        }
        found.sort_by(|a, b| a.meta.session_id.cmp(&b.meta.session_id));
        Ok(found)
    }
}

impl JournalRecoveryManager {
    /// Replays all intact CRC32-verified operations for `session_id`, safely stopping at any torn or
    /// corrupted trailing bytes.
    pub fn recover_session<Op: DeserializeOwned>(
        &self,
        session_id: &str,
    ) -> Result<RecoveredJournal<Op>, JournalError> {
        let session_dir = self.data_dir.join("journals").join(session_id);
        let meta_path = session_dir.join("meta.toml");
        if !meta_path.exists() {
            return Err(JournalError::NotFound(session_id.to_string()));
        }
        let meta_raw = fs::read_to_string(&meta_path).map_err(|source| JournalError::Io {
            path: meta_path.clone(),
            source,
        })?;
        let meta: JournalSessionMeta =
            toml::from_str(&meta_raw).map_err(|e| JournalError::Serialization(e.to_string()))?;

        let journal_path = session_dir.join("ops.journal");
        let (operations, skipped_tail_bytes) = read_journal_file(&journal_path)?;
        Ok(RecoveredJournal {
            meta,
            operations,
            skipped_tail_bytes,
        })
    }

    /// Removes a recovered or user-dismissed session directory from `<data_dir>/journals/`.
    pub fn discard_session(&self, session_id: &str) -> Result<(), JournalError> {
        let session_dir = self.data_dir.join("journals").join(session_id);
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).map_err(|source| JournalError::Io {
                path: session_dir,
                source,
            })?;
        }
        Ok(())
    }
}

/// Reads and CRC32-validates an `ops.journal` file, safely halting at any truncated or corrupted tail frame.
pub fn read_journal_file<Op: DeserializeOwned>(
    journal_path: &Path,
) -> Result<(Vec<(u64, Op)>, u64), JournalError> {
    let mut f = File::open(journal_path).map_err(|source| JournalError::Io {
        path: journal_path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)
        .map_err(|source| JournalError::Io {
            path: journal_path.to_path_buf(),
            source,
        })?;

    if bytes.len() < JOURNAL_MAGIC.len() || &bytes[..JOURNAL_MAGIC.len()] != JOURNAL_MAGIC {
        return Err(JournalError::InvalidHeader {
            path: journal_path.to_path_buf(),
        });
    }

    let mut offset = JOURNAL_MAGIC.len();
    let mut ops = Vec::new();

    while offset < bytes.len() {
        let remaining = bytes.len() - offset;
        if remaining < FRAME_HEADER_LEN {
            break;
        }
        let hdr = &bytes[offset..offset + FRAME_HEADER_LEN];
        let seq = u64::from_le_bytes([
            hdr[0], hdr[1], hdr[2], hdr[3], hdr[4], hdr[5], hdr[6], hdr[7],
        ]);
        let ts = i64::from_le_bytes([
            hdr[8], hdr[9], hdr[10], hdr[11], hdr[12], hdr[13], hdr[14], hdr[15],
        ]);
        let payload_len = u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]);
        let expected_crc = u32::from_le_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]);

        let len_usize = payload_len as usize;
        if len_usize > MAX_FRAME_PAYLOAD_BYTES || remaining < FRAME_HEADER_LEN + len_usize {
            break;
        }

        let payload = &bytes[offset + FRAME_HEADER_LEN..offset + FRAME_HEADER_LEN + len_usize];
        let actual_crc = compute_frame_crc32(seq, ts, payload_len, payload);
        if actual_crc != expected_crc {
            break;
        }

        let Ok(op) = postcard::from_bytes::<Op>(payload) else {
            break;
        };
        ops.push((seq, op));
        offset += FRAME_HEADER_LEN + len_usize;
    }

    let skipped_tail_bytes = (bytes.len() - offset) as u64;
    Ok((ops, skipped_tail_bytes))
}

fn compute_frame_crc32(seq: u64, timestamp_ms: i64, payload_len: u32, payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(&seq.to_le_bytes());
    hasher.update(&timestamp_ms.to_le_bytes());
    hasher.update(&payload_len.to_le_bytes());
    hasher.update(payload);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    enum SampleOp {
        SetCell { row: u32, col: u32, val: String },
        ResizeCol { col: u32, width_bits: u32 },
    }

    #[test]
    fn simulated_kill_recovers_up_to_last_flushed_op_and_skips_corrupted_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let meta = JournalSessionMeta::new("sess-1", "Budget.xlsx", None);
        let policy = JournalFlushPolicy::new(10, Duration::from_secs(3600));

        // Append 25 operations: ops 1..=20 auto-flush (at 10 and 20), while 21..=25 remain unflushed in memory.
        let journal_file_path = {
            let mut writer = JournalWriter::<SampleOp>::open(dir.path(), &meta, policy)
                .expect("open journal writer");
            for i in 1..=25u32 {
                writer
                    .append(&SampleOp::SetCell {
                        row: i,
                        col: 0,
                        val: format!("row-{i}"),
                    })
                    .expect("append op");
            }
            assert_eq!(writer.flushed_seq(), 20);
            let p = writer.journal_path().to_path_buf();
            // Drop without `close_cleanly()` to simulate `kill -9` abrupt termination!
            drop(writer);
            p
        };

        // Restart: detect unclean session and verify recovery stops at op 20.
        let mgr = JournalRecoveryManager::with_data_dir(dir.path());
        let sessions = mgr.scan_unclean_sessions().expect("scan sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].meta.document_title, "Budget.xlsx");

        let recovered: RecoveredJournal<SampleOp> =
            mgr.recover_session("sess-1").expect("recover clean tail");
        assert_eq!(recovered.operations.len(), 20);
        assert_eq!(recovered.skipped_tail_bytes, 0);
        assert_eq!(recovered.operations[19].0, 20);
        assert_eq!(
            recovered.operations[19].1,
            SampleOp::SetCell {
                row: 20,
                col: 0,
                val: "row-20".to_string()
            }
        );

        // Now simulate power-loss torn write + bit-rot at the tail of `ops.journal`!
        let mut f = OpenOptions::new()
            .append(true)
            .open(&journal_file_path)
            .expect("open journal for tail corruption");
        f.write_all(&[0x15, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF, 0x99])
            .expect("append torn tail bytes");
        drop(f);

        let recovered_corrupt: RecoveredJournal<SampleOp> = mgr
            .recover_session("sess-1")
            .expect("recover with torn tail");
        assert_eq!(recovered_corrupt.operations.len(), 20);
        assert_eq!(recovered_corrupt.skipped_tail_bytes, 9);

        // Clean close on a fresh session leaves zero unclean sessions.
        mgr.discard_session("sess-1").expect("discard");
        let meta2 = JournalSessionMeta::new("sess-2", "Clean.xlsx", None);
        let mut writer2 =
            JournalWriter::<SampleOp>::open(dir.path(), &meta2, policy).expect("open sess-2");
        writer2
            .append(&SampleOp::ResizeCol {
                col: 1,
                width_bits: 120,
            })
            .expect("append");
        writer2.close_cleanly().expect("clean close");
        assert!(mgr.scan_unclean_sessions().expect("rescan").is_empty());
    }
}
