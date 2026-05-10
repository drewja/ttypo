//! Persistent per-document typing progress.
//!
//! Stored as a single TOML file at `<data_dir>/progress.toml`. Each entry is
//! keyed by the canonicalized absolute path of the source file and carries a
//! SHA-256 content hash so stale or modified files can be detected on resume.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
#[cfg(test)]
use std::io::Read;
use std::path::{Path, PathBuf};
use crate::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const FILENAME: &str = "progress.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub content_hash: String,
    pub word_index: usize,
    #[serde(default)]
    pub total_words: usize,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub source_label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct FileFormat {
    version: u32,
    // BTreeMap for deterministic on-disk ordering (easier to diff/inspect).
    documents: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone)]
pub struct ProgressStore {
    dir: PathBuf,
    documents: BTreeMap<String, Entry>,
    // Set when the on-disk file declares a newer schema than this binary
    // understands. We refuse to save in that case so we don't overwrite a
    // future-version file with our older format.
    read_only: bool,
}

impl ProgressStore {
    /// Load the store from `<dir>/progress.toml`. Returns an empty store if
    /// the file is missing or malformed. If the file declares a newer schema
    /// than this binary supports, returns an empty read-only store so future
    /// saves don't downgrade it. Persistence is best-effort.
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join(FILENAME);
        let (documents, read_only) = match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<FileFormat>(&text) {
                Ok(f) if f.version <= SCHEMA_VERSION => (f.documents, false),
                Ok(_) => (BTreeMap::new(), true),
                Err(_) => (BTreeMap::new(), false),
            },
            Err(_) => (BTreeMap::new(), false),
        };
        Self {
            dir,
            documents,
            read_only,
        }
    }

    pub fn lookup(&self, canonical_path: &Path) -> Option<&Entry> {
        self.documents.get(&path_key(canonical_path))
    }

    pub fn upsert(&mut self, canonical_path: &Path, entry: Entry) {
        self.documents.insert(path_key(canonical_path), entry);
    }

    pub fn remove(&mut self, canonical_path: &Path) {
        self.documents.remove(&path_key(canonical_path));
    }

    /// Serialize the store to disk. Creates the data directory if missing.
    /// No-op when the store was loaded from a newer-schema file.
    pub fn save(&self) -> io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        let file = FileFormat {
            version: SCHEMA_VERSION,
            documents: self.documents.clone(),
        };
        let text = toml::to_string(&file).map_err(io::Error::other)?;
        fs::write(self.dir.join(FILENAME), text)
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Canonicalize a file path, falling back to the raw path on failure.
pub fn canonicalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Hex-encoded SHA-256 hash of the given bytes. Prefer this when the file
/// has already been loaded (e.g. via mmap) to avoid a redundant read.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// Hex-encoded SHA-256 hash of a file's bytes, streamed so we never hold the
// whole file in memory at once. The mmap-backed flow prefers hash_bytes over
// the already-mapped slice; this exists for tests that only have a path.
#[cfg(test)]
fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Current unix timestamp in seconds (0 on clock failure).
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> Entry {
        Entry {
            content_hash: "abc123".into(),
            word_index: 42,
            total_words: 1000,
            updated_at: 1_700_000_000,
            source_label: "book.txt".into(),
        }
    }

    #[test]
    fn load_missing_file_returns_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProgressStore::load(dir.path().join("nonexistent"));
        assert!(store.documents.is_empty());
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();

        let mut store = ProgressStore::load(data_dir.clone());
        let fake_path = PathBuf::from("/tmp/fake/book.txt");
        store.upsert(&fake_path, sample_entry());
        store.save().unwrap();

        let reloaded = ProgressStore::load(data_dir);
        assert_eq!(reloaded.lookup(&fake_path), Some(&sample_entry()));
    }

    #[test]
    fn remove_clears_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgressStore::load(dir.path().to_path_buf());
        let p = PathBuf::from("/tmp/x.txt");
        store.upsert(&p, sample_entry());
        assert!(store.lookup(&p).is_some());
        store.remove(&p);
        assert!(store.lookup(&p).is_none());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        let path = dir.path().join(FILENAME);
        let toml_with_future_field = r#"
version = 1
future_top_level = "ignored"

[documents."/tmp/book.txt"]
content_hash = "deadbeef"
word_index = 7
total_words = 100
updated_at = 1700000000
source_label = "book.txt"
future_entry_field = true
"#;
        fs::write(&path, toml_with_future_field).unwrap();

        let store = ProgressStore::load(dir.path().to_path_buf());
        let entry = store.lookup(Path::new("/tmp/book.txt")).unwrap();
        assert_eq!(entry.word_index, 7);
        assert_eq!(entry.content_hash, "deadbeef");
    }

    #[test]
    fn future_schema_loads_empty_and_save_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILENAME);
        let future = format!(
            "version = {}\n\n[documents.\"/tmp/x.txt\"]\ncontent_hash = \"a\"\nword_index = 1\n",
            SCHEMA_VERSION + 1,
        );
        fs::write(&path, &future).unwrap();

        let mut store = ProgressStore::load(dir.path().to_path_buf());
        assert!(store.lookup(Path::new("/tmp/x.txt")).is_none());

        // Mutate and save: must NOT overwrite the future-version file.
        store.upsert(Path::new("/tmp/y.txt"), sample_entry());
        store.save().unwrap();
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk, future, "future-version file must be preserved");
    }

    #[test]
    fn hash_file_is_deterministic_and_hex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.txt");
        fs::write(&path, b"hello world").unwrap();
        let a = hash_file(&path).unwrap();
        let b = hash_file(&path).unwrap();
        assert_eq!(a, b);
        // SHA-256 is 64 hex chars.
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
