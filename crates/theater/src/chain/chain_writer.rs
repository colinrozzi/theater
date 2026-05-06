//! # Chain Writer
//!
//! Streams actor events to disk in the EVENT format for crash recovery and debugging.
//! Each actor run gets its own chain file at `/tmp/theater/chains/{actor_id}.chain`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::chain::ChainEvent;
use crate::TheaterId;

/// Metadata about a run, written to `{actor_id}.meta.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub actor_id: String,
    pub actor_name: Option<String>,
    pub manifest_path: Option<String>,
    pub started_at: u64,
}

/// Writes events to a `.chain` file using the EVENT format.
///
/// The EVENT format is:
/// ```text
/// EVENT <hash-hex>
/// <parent-hash-hex or 0000000000000000>
/// <event-type>
/// <body-length>
///
/// <body>
///
/// ```
pub struct ChainWriter {
    writer: BufWriter<File>,
    path: PathBuf,
}

impl ChainWriter {
    /// Creates a new chain writer for the given actor.
    ///
    /// If `dir` is provided, writes there. Otherwise defaults to `/tmp/theater/chains/`.
    pub fn new(actor_id: &TheaterId, dir: Option<&PathBuf>) -> Result<Self> {
        let chains_dir = dir.cloned().unwrap_or_else(Self::default_dir);
        fs::create_dir_all(&chains_dir)
            .with_context(|| format!("Failed to create chains directory: {:?}", chains_dir))?;

        let path = chains_dir.join(format!("{}.chain", actor_id));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open chain file: {:?}", path))?;

        Ok(Self {
            writer: BufWriter::new(file),
            path,
        })
    }

    /// Returns the default directory where chain files are stored.
    pub fn default_dir() -> PathBuf {
        PathBuf::from("/tmp/theater/chains")
    }

    /// Writes metadata about this run.
    pub fn write_meta(actor_id: &TheaterId, meta: &RunMeta) -> Result<()> {
        let chains_dir = Self::default_dir();
        fs::create_dir_all(&chains_dir)?;

        let path = chains_dir.join(format!("{}.meta.json", actor_id));
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Appends an event to the chain file in human-readable format.
    pub fn append(&mut self, event: &ChainEvent) -> Result<()> {
        let formatted = super::format::format_event(event);
        write!(self.writer, "{}", formatted)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Returns the path to this chain file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for ChainWriter {
    fn drop(&mut self) {
        // Ensure everything is flushed on drop
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_chain_writer_append() {
        let actor_id = TheaterId::generate();
        let mut writer = ChainWriter::new(&actor_id, None).unwrap();

        let event = ChainEvent {
            hash: vec![0x1a, 0x2b, 0x3c],
            parent_hash: None,
            event_type: "test".to_string(),
            data: b"not valid cgrf".to_vec(),
        };

        writer.append(&event).unwrap();
        drop(writer);

        // Read back
        let path = ChainWriter::default_dir().join(format!("{}.chain", actor_id));
        let mut file = File::open(&path).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();

        assert!(contents.contains("---"));
        assert!(contents.contains("hash: 1a2b3c"));
        assert!(contents.contains("parent: root"));

        // Cleanup
        fs::remove_file(&path).ok();
    }
}
