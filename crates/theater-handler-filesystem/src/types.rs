//! Type definitions for filesystem handler

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileSystemError {
    #[error("Path error: {0}")]
    PathError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum FileSystemCommand {
    ReadFile { path: String },
    WriteFile { path: String, contents: String },
    ListFiles { path: String },
    DeleteFile { path: String },
    CreateDir { path: String },
    DeleteDir { path: String },
    PathExists { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum FileSystemResponse {
    ReadFile(Result<Vec<u8>, String>),
    WriteFile(Result<(), String>),
    ListFiles(Result<Vec<String>, String>),
    DeleteFile(Result<(), String>),
    CreateDir(Result<(), String>),
    DeleteDir(Result<(), String>),
    PathExists(Result<bool, String>),
}

// ============================================================================
// WASI Filesystem Resource Types
// ============================================================================

/// Represents a file or directory descriptor
///
/// This is the backing type for the WASI descriptor resource.
/// It holds the actual file handle and path information.
#[derive(Debug)]
pub struct Descriptor {
    /// The absolute path this descriptor refers to
    pub path: PathBuf,
    /// The type of descriptor (file or directory)
    pub descriptor_type: DescriptorType,
    /// File handle for file descriptors (None for directories)
    pub file: Option<Arc<Mutex<File>>>,
    /// The flags this descriptor was opened with
    pub flags: DescriptorFlags,
    /// Whether this is a preopened directory
    pub is_preopen: bool,
}

/// Type of a filesystem descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorType {
    Unknown,
    BlockDevice,
    CharacterDevice,
    Directory,
    Fifo,
    SymbolicLink,
    RegularFile,
    Socket,
}

impl Default for DescriptorType {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Flags for a descriptor
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorFlags {
    pub read: bool,
    pub write: bool,
    pub file_integrity_sync: bool,
    pub data_integrity_sync: bool,
    pub requested_write_sync: bool,
    pub mutate_directory: bool,
}

impl Descriptor {
    /// Create a new directory descriptor (for preopens)
    pub fn new_directory(path: PathBuf, is_preopen: bool) -> Self {
        Self {
            path,
            descriptor_type: DescriptorType::Directory,
            file: None,
            flags: DescriptorFlags {
                read: true,
                write: false,
                mutate_directory: false,
                ..Default::default()
            },
            is_preopen,
        }
    }

    /// Create a new file descriptor
    pub fn new_file(path: PathBuf, file: File, flags: DescriptorFlags) -> Self {
        Self {
            path,
            descriptor_type: DescriptorType::RegularFile,
            file: Some(Arc::new(Mutex::new(file))),
            flags,
            is_preopen: false,
        }
    }

    /// Get the type of this descriptor
    pub fn get_type(&self) -> DescriptorType {
        self.descriptor_type
    }

    /// Check if this is a directory
    pub fn is_directory(&self) -> bool {
        self.descriptor_type == DescriptorType::Directory
    }

    /// Check if this is a file
    pub fn is_file(&self) -> bool {
        self.descriptor_type == DescriptorType::RegularFile
    }

    /// Read data from the descriptor at a given offset
    pub fn read(&self, length: u64, offset: u64) -> Result<(Vec<u8>, bool), std::io::Error> {
        if let Some(file) = &self.file {
            let mut file = file.lock().unwrap();
            file.seek(SeekFrom::Start(offset))?;
            let mut buffer = vec![0u8; length as usize];
            let bytes_read = file.read(&mut buffer)?;
            buffer.truncate(bytes_read);
            let at_end = bytes_read < length as usize;
            Ok((buffer, at_end))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Not a file descriptor",
            ))
        }
    }

    /// Write data to the descriptor at a given offset
    pub fn write(&self, data: &[u8], offset: u64) -> Result<u64, std::io::Error> {
        if let Some(file) = &self.file {
            let mut file = file.lock().unwrap();
            file.seek(SeekFrom::Start(offset))?;
            let bytes_written = file.write(data)?;
            Ok(bytes_written as u64)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Not a file descriptor",
            ))
        }
    }

    /// Sync data to disk
    pub fn sync(&self) -> Result<(), std::io::Error> {
        if let Some(file) = &self.file {
            let file = file.lock().unwrap();
            file.sync_all()?;
        }
        Ok(())
    }

    /// Sync data (not metadata) to disk
    pub fn sync_data(&self) -> Result<(), std::io::Error> {
        if let Some(file) = &self.file {
            let file = file.lock().unwrap();
            file.sync_data()?;
        }
        Ok(())
    }

    /// Get file size
    pub fn get_size(&self) -> Result<u64, std::io::Error> {
        let metadata = fs::metadata(&self.path)?;
        Ok(metadata.len())
    }

    /// Set file size (truncate or extend)
    pub fn set_size(&self, size: u64) -> Result<(), std::io::Error> {
        if let Some(file) = &self.file {
            let file = file.lock().unwrap();
            file.set_len(size)?;
        }
        Ok(())
    }

    /// Open a file relative to this directory descriptor
    pub fn open_at(
        &self,
        path: &str,
        flags: OpenFlags,
        desc_flags: DescriptorFlags,
    ) -> Result<Descriptor, std::io::Error> {
        if !self.is_directory() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Not a directory",
            ));
        }

        let full_path = self.path.join(path);

        // Determine if we're opening a file or directory
        if flags.directory {
            // Opening a directory
            if !full_path.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "Not a directory",
                ));
            }
            Ok(Descriptor::new_directory(full_path, false))
        } else {
            // Opening a file
            let mut open_options = OpenOptions::new();
            if desc_flags.read {
                open_options.read(true);
            }
            if desc_flags.write {
                open_options.write(true);
            }
            if flags.create {
                open_options.create(true);
            }
            if flags.exclusive {
                open_options.create_new(true);
            }
            if flags.truncate {
                open_options.truncate(true);
            }

            let file = open_options.open(&full_path)?;
            Ok(Descriptor::new_file(full_path, file, desc_flags))
        }
    }

    /// Create a directory relative to this directory descriptor
    pub fn create_directory_at(&self, path: &str) -> Result<(), std::io::Error> {
        if !self.is_directory() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Not a directory",
            ));
        }
        let full_path = self.path.join(path);
        fs::create_dir(&full_path)
    }

    /// Remove a directory relative to this directory descriptor
    pub fn remove_directory_at(&self, path: &str) -> Result<(), std::io::Error> {
        if !self.is_directory() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Not a directory",
            ));
        }
        let full_path = self.path.join(path);
        fs::remove_dir(&full_path)
    }

    /// Unlink a file relative to this directory descriptor
    pub fn unlink_file_at(&self, path: &str) -> Result<(), std::io::Error> {
        if !self.is_directory() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Not a directory",
            ));
        }
        let full_path = self.path.join(path);
        fs::remove_file(&full_path)
    }

    /// Read directory entries
    pub fn read_directory(&self) -> Result<DirectoryEntryStream, std::io::Error> {
        if !self.is_directory() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "Not a directory",
            ));
        }
        DirectoryEntryStream::new(&self.path)
    }
}

/// Flags for opening files
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenFlags {
    pub create: bool,
    pub directory: bool,
    pub exclusive: bool,
    pub truncate: bool,
}

/// Stream for reading directory entries
#[derive(Debug)]
pub struct DirectoryEntryStream {
    /// The entries in the directory
    entries: Vec<DirectoryEntry>,
    /// Current position in the stream
    position: usize,
}

/// A single directory entry
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// Type of the entry
    pub entry_type: DescriptorType,
    /// Name of the entry
    pub name: String,
}

impl DirectoryEntryStream {
    /// Create a new directory entry stream from a path
    pub fn new(path: &PathBuf) -> Result<Self, std::io::Error> {
        let mut entries = Vec::new();

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let entry_type = if file_type.is_dir() {
                DescriptorType::Directory
            } else if file_type.is_file() {
                DescriptorType::RegularFile
            } else if file_type.is_symlink() {
                DescriptorType::SymbolicLink
            } else {
                DescriptorType::Unknown
            };

            entries.push(DirectoryEntry {
                entry_type,
                name: entry.file_name().to_string_lossy().to_string(),
            });
        }

        Ok(Self {
            entries,
            position: 0,
        })
    }

    /// Read the next directory entry
    pub fn read_entry(&mut self) -> Option<DirectoryEntry> {
        if self.position < self.entries.len() {
            let entry = self.entries[self.position].clone();
            self.position += 1;
            Some(entry)
        } else {
            None
        }
    }
}
