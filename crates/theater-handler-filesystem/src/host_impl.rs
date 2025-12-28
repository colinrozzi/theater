//! Host trait implementations for WASI filesystem interfaces
//!
//! This module implements the bindgen-generated Host traits for the WASI filesystem interfaces.
//! The backing types are defined in types.rs and mapped via bindgen's `with` option.

use crate::bindings::wasi::filesystem::preopens::Host as PreopensHost;
use crate::bindings::wasi::filesystem::types::{
    self, Advice, DescriptorFlags as WasiDescriptorFlags, DescriptorStat, DescriptorType as WasiDescriptorType,
    DirectoryEntry as WasiDirectoryEntry, ErrorCode, Host as FilesystemTypesHost, HostDescriptor,
    HostDirectoryEntryStream, MetadataHashValue, NewTimestamp, OpenFlags as WasiOpenFlags,
    PathFlags,
};
use crate::events::FilesystemEventData;
use crate::types::{
    Descriptor, DescriptorFlags, DescriptorType, DirectoryEntryStream, OpenFlags,
};

use std::fs;
use std::path::PathBuf;
use theater::actor::ActorStore;
use theater::events::EventPayload;
use theater_handler_io::{InputStream, IoError, OutputStream};
use tracing::debug;
use wasmtime::component::Resource;

// ============================================================================
// Helper functions
// ============================================================================

/// Convert our DescriptorType to WASI DescriptorType
fn to_wasi_descriptor_type(dt: DescriptorType) -> WasiDescriptorType {
    match dt {
        DescriptorType::Unknown => WasiDescriptorType::Unknown,
        DescriptorType::BlockDevice => WasiDescriptorType::BlockDevice,
        DescriptorType::CharacterDevice => WasiDescriptorType::CharacterDevice,
        DescriptorType::Directory => WasiDescriptorType::Directory,
        DescriptorType::Fifo => WasiDescriptorType::Fifo,
        DescriptorType::SymbolicLink => WasiDescriptorType::SymbolicLink,
        DescriptorType::RegularFile => WasiDescriptorType::RegularFile,
        DescriptorType::Socket => WasiDescriptorType::Socket,
    }
}

/// Convert WASI OpenFlags to our OpenFlags
fn from_wasi_open_flags(flags: WasiOpenFlags) -> OpenFlags {
    OpenFlags {
        create: flags.contains(WasiOpenFlags::CREATE),
        directory: flags.contains(WasiOpenFlags::DIRECTORY),
        exclusive: flags.contains(WasiOpenFlags::EXCLUSIVE),
        truncate: flags.contains(WasiOpenFlags::TRUNCATE),
    }
}

/// Convert WASI DescriptorFlags to our DescriptorFlags
fn from_wasi_descriptor_flags(flags: WasiDescriptorFlags) -> DescriptorFlags {
    DescriptorFlags {
        read: flags.contains(WasiDescriptorFlags::READ),
        write: flags.contains(WasiDescriptorFlags::WRITE),
        file_integrity_sync: flags.contains(WasiDescriptorFlags::FILE_INTEGRITY_SYNC),
        data_integrity_sync: flags.contains(WasiDescriptorFlags::DATA_INTEGRITY_SYNC),
        requested_write_sync: flags.contains(WasiDescriptorFlags::REQUESTED_WRITE_SYNC),
        mutate_directory: flags.contains(WasiDescriptorFlags::MUTATE_DIRECTORY),
    }
}

/// Convert our DescriptorFlags to WASI DescriptorFlags
fn to_wasi_descriptor_flags(flags: DescriptorFlags) -> WasiDescriptorFlags {
    let mut wasi_flags = WasiDescriptorFlags::empty();
    if flags.read {
        wasi_flags |= WasiDescriptorFlags::READ;
    }
    if flags.write {
        wasi_flags |= WasiDescriptorFlags::WRITE;
    }
    if flags.file_integrity_sync {
        wasi_flags |= WasiDescriptorFlags::FILE_INTEGRITY_SYNC;
    }
    if flags.data_integrity_sync {
        wasi_flags |= WasiDescriptorFlags::DATA_INTEGRITY_SYNC;
    }
    if flags.requested_write_sync {
        wasi_flags |= WasiDescriptorFlags::REQUESTED_WRITE_SYNC;
    }
    if flags.mutate_directory {
        wasi_flags |= WasiDescriptorFlags::MUTATE_DIRECTORY;
    }
    wasi_flags
}

/// Convert std::io::Error to WASI ErrorCode
fn io_error_to_error_code(e: std::io::Error) -> ErrorCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::NoEntry,
        std::io::ErrorKind::PermissionDenied => ErrorCode::Access,
        std::io::ErrorKind::AlreadyExists => ErrorCode::Exist,
        std::io::ErrorKind::InvalidInput => ErrorCode::Invalid,
        std::io::ErrorKind::InvalidData => ErrorCode::Invalid,
        std::io::ErrorKind::WouldBlock => ErrorCode::WouldBlock,
        std::io::ErrorKind::Interrupted => ErrorCode::Interrupted,
        std::io::ErrorKind::OutOfMemory => ErrorCode::InsufficientMemory,
        std::io::ErrorKind::NotADirectory => ErrorCode::NotDirectory,
        std::io::ErrorKind::IsADirectory => ErrorCode::IsDirectory,
        std::io::ErrorKind::DirectoryNotEmpty => ErrorCode::NotEmpty,
        std::io::ErrorKind::ReadOnlyFilesystem => ErrorCode::ReadOnly,
        std::io::ErrorKind::CrossesDevices => ErrorCode::CrossDevice,
        std::io::ErrorKind::TooManyLinks => ErrorCode::TooManyLinks,
        std::io::ErrorKind::InvalidFilename => ErrorCode::NameTooLong,
        std::io::ErrorKind::FileTooLarge => ErrorCode::FileTooLarge,
        std::io::ErrorKind::StorageFull => ErrorCode::InsufficientSpace,
        _ => ErrorCode::Io,
    }
}

// ============================================================================
// HostDescriptor implementation
// ============================================================================

impl<E> HostDescriptor for ActorStore<E>
where
    E: EventPayload + Clone + From<FilesystemEventData> + Send + 'static,
{
    async fn read_via_stream(
        &mut self,
        self_: Resource<Descriptor>,
        offset: types::Filesize,
    ) -> wasmtime::Result<Result<Resource<InputStream>, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.read-via-stream offset={}", offset);

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        if !descriptor.is_file() {
            return Ok(Err(ErrorCode::IsDirectory));
        }

        // Read all remaining content from offset
        match descriptor.read(u64::MAX, offset) {
            Ok((data, _)) => {
                drop(table);
                let stream = InputStream::from_bytes(data);
                let resource = self.resource_table.lock().unwrap().push(stream)?;
                Ok(Ok(resource))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn write_via_stream(
        &mut self,
        self_: Resource<Descriptor>,
        _offset: types::Filesize,
    ) -> wasmtime::Result<Result<Resource<OutputStream>, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.write-via-stream");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        if !descriptor.is_file() {
            return Ok(Err(ErrorCode::IsDirectory));
        }

        if !descriptor.flags.write {
            return Ok(Err(ErrorCode::Access));
        }

        drop(table);
        let stream = OutputStream::new();
        let resource = self.resource_table.lock().unwrap().push(stream)?;
        Ok(Ok(resource))
    }

    async fn append_via_stream(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<Resource<OutputStream>, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.append-via-stream");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        if !descriptor.is_file() {
            return Ok(Err(ErrorCode::IsDirectory));
        }

        if !descriptor.flags.write {
            return Ok(Err(ErrorCode::Access));
        }

        drop(table);
        let stream = OutputStream::new();
        let resource = self.resource_table.lock().unwrap().push(stream)?;
        Ok(Ok(resource))
    }

    async fn advise(
        &mut self,
        _self_: Resource<Descriptor>,
        _offset: types::Filesize,
        _length: types::Filesize,
        _advice: Advice,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.advise");
        // Advice is a hint - we can ignore it
        Ok(Ok(()))
    }

    async fn sync_data(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.sync-data");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.sync_data() {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn get_flags(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<WasiDescriptorFlags, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.get-flags");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        Ok(Ok(to_wasi_descriptor_flags(descriptor.flags)))
    }

    async fn get_type(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<WasiDescriptorType, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.get-type");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        Ok(Ok(to_wasi_descriptor_type(descriptor.descriptor_type)))
    }

    async fn set_size(
        &mut self,
        self_: Resource<Descriptor>,
        size: types::Filesize,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.set-size size={}", size);

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        if !descriptor.flags.write {
            return Ok(Err(ErrorCode::Access));
        }

        match descriptor.set_size(size) {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn set_times(
        &mut self,
        self_: Resource<Descriptor>,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.set-times");
        // TODO: Implement timestamp modification
        Ok(Ok(()))
    }

    async fn read(
        &mut self,
        self_: Resource<Descriptor>,
        length: types::Filesize,
        offset: types::Filesize,
    ) -> wasmtime::Result<Result<(Vec<u8>, bool), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.read length={} offset={}", length, offset);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.read".to_string(),
            FilesystemEventData::ReadCall {
                path: "".to_string(),
                offset,
                length,
            },
            Some(format!("Reading {} bytes at offset {}", length, offset)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.read(length, offset) {
            Ok((data, at_end)) => {
                let bytes_read = data.len();
                drop(table);

                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.read".to_string(),
                    FilesystemEventData::ReadResult {
                        bytes_read,
                        success: true,
                    },
                    Some(format!("Read {} bytes, at_end={}", bytes_read, at_end)),
                );

                Ok(Ok((data, at_end)))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn write(
        &mut self,
        self_: Resource<Descriptor>,
        buffer: Vec<u8>,
        offset: types::Filesize,
    ) -> wasmtime::Result<Result<types::Filesize, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.write len={} offset={}", buffer.len(), offset);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.write".to_string(),
            FilesystemEventData::WriteCall {
                path: "".to_string(),
                size: buffer.len(),
            },
            Some(format!("Writing {} bytes at offset {}", buffer.len(), offset)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        if !descriptor.flags.write {
            return Ok(Err(ErrorCode::Access));
        }

        match descriptor.write(&buffer, offset) {
            Ok(bytes_written) => {
                drop(table);

                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.write".to_string(),
                    FilesystemEventData::WriteResult {
                        bytes_written: bytes_written as usize,
                        success: true,
                    },
                    Some(format!("Wrote {} bytes", bytes_written)),
                );

                Ok(Ok(bytes_written))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn read_directory(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<Resource<DirectoryEntryStream>, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.read-directory");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.read_directory() {
            Ok(stream) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(stream)?;
                Ok(Ok(resource))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn sync(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.sync");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.sync() {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn create_directory_at(
        &mut self,
        self_: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.create-directory-at path={}", path);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.create-directory-at".to_string(),
            FilesystemEventData::CreateDirCall { path: path.clone() },
            Some(format!("Creating directory: {}", path)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.create_directory_at(&path) {
            Ok(()) => {
                drop(table);
                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.create-directory-at".to_string(),
                    FilesystemEventData::CreateDirResult { success: true },
                    Some("Directory created successfully".to_string()),
                );
                Ok(Ok(()))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn stat(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.stat");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match fs::metadata(&descriptor.path) {
            Ok(metadata) => {
                let stat = DescriptorStat {
                    type_: to_wasi_descriptor_type(descriptor.descriptor_type),
                    link_count: metadata.nlink(),
                    size: metadata.len(),
                    data_access_timestamp: None,
                    data_modification_timestamp: None,
                    status_change_timestamp: None,
                };
                Ok(Ok(stat))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn stat_at(
        &mut self,
        self_: Resource<Descriptor>,
        _path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.stat-at path={}", path);

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        let full_path = descriptor.path.join(&path);

        match fs::metadata(&full_path) {
            Ok(metadata) => {
                let dtype = if metadata.is_dir() {
                    WasiDescriptorType::Directory
                } else if metadata.is_file() {
                    WasiDescriptorType::RegularFile
                } else if metadata.is_symlink() {
                    WasiDescriptorType::SymbolicLink
                } else {
                    WasiDescriptorType::Unknown
                };

                let stat = DescriptorStat {
                    type_: dtype,
                    link_count: metadata.nlink(),
                    size: metadata.len(),
                    data_access_timestamp: None,
                    data_modification_timestamp: None,
                    status_change_timestamp: None,
                };
                Ok(Ok(stat))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn set_times_at(
        &mut self,
        _self_: Resource<Descriptor>,
        _path_flags: PathFlags,
        _path: String,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.set-times-at");
        // TODO: Implement timestamp modification
        Ok(Ok(()))
    }

    async fn link_at(
        &mut self,
        _self_: Resource<Descriptor>,
        _old_path_flags: PathFlags,
        _old_path: String,
        _new_descriptor: Resource<Descriptor>,
        _new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.link-at");
        // TODO: Implement hard links
        Ok(Err(ErrorCode::Unsupported))
    }

    async fn open_at(
        &mut self,
        self_: Resource<Descriptor>,
        _path_flags: PathFlags,
        path: String,
        open_flags: WasiOpenFlags,
        flags: WasiDescriptorFlags,
    ) -> wasmtime::Result<Result<Resource<Descriptor>, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.open-at path={}", path);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.open-at".to_string(),
            FilesystemEventData::OpenAtCall {
                path: path.clone(),
                flags: format!("{:?}", open_flags),
            },
            Some(format!("Opening path: {}", path)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        let our_open_flags = from_wasi_open_flags(open_flags);
        let our_desc_flags = from_wasi_descriptor_flags(flags);

        match descriptor.open_at(&path, our_open_flags, our_desc_flags) {
            Ok(new_descriptor) => {
                drop(table);
                let resource = self.resource_table.lock().unwrap().push(new_descriptor)?;

                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.open-at".to_string(),
                    FilesystemEventData::OpenAtResult { success: true },
                    Some("Opened successfully".to_string()),
                );

                Ok(Ok(resource))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn readlink_at(
        &mut self,
        self_: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<String, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.readlink-at path={}", path);

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        let full_path = descriptor.path.join(&path);

        match fs::read_link(&full_path) {
            Ok(target) => Ok(Ok(target.to_string_lossy().to_string())),
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn remove_directory_at(
        &mut self,
        self_: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.remove-directory-at path={}", path);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.remove-directory-at".to_string(),
            FilesystemEventData::DeleteDirCall { path: path.clone() },
            Some(format!("Removing directory: {}", path)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.remove_directory_at(&path) {
            Ok(()) => {
                drop(table);
                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.remove-directory-at".to_string(),
                    FilesystemEventData::DeleteDirResult { success: true },
                    Some("Directory removed successfully".to_string()),
                );
                Ok(Ok(()))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn rename_at(
        &mut self,
        self_: Resource<Descriptor>,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.rename-at {} -> {}", old_path, new_path);

        let table = self.resource_table.lock().unwrap();
        let src_descriptor: &Descriptor = table.get(&self_)?;
        let dst_descriptor: &Descriptor = table.get(&new_descriptor)?;

        let src_full_path = src_descriptor.path.join(&old_path);
        let dst_full_path = dst_descriptor.path.join(&new_path);

        drop(table);

        match fs::rename(&src_full_path, &dst_full_path) {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn symlink_at(
        &mut self,
        _self_: Resource<Descriptor>,
        _old_path: String,
        _new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.symlink-at");
        // TODO: Implement symlinks
        Ok(Err(ErrorCode::Unsupported))
    }

    async fn unlink_file_at(
        &mut self,
        self_: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.unlink-file-at path={}", path);

        self.record_handler_event(
            "wasi:filesystem/types/descriptor.unlink-file-at".to_string(),
            FilesystemEventData::DeleteFileCall { path: path.clone() },
            Some(format!("Unlinking file: {}", path)),
        );

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match descriptor.unlink_file_at(&path) {
            Ok(()) => {
                drop(table);
                self.record_handler_event(
                    "wasi:filesystem/types/descriptor.unlink-file-at".to_string(),
                    FilesystemEventData::DeleteFileResult { success: true },
                    Some("File unlinked successfully".to_string()),
                );
                Ok(Ok(()))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn is_same_object(
        &mut self,
        self_: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        debug!("wasi:filesystem/types/descriptor.is-same-object");

        let table = self.resource_table.lock().unwrap();
        let desc1: &Descriptor = table.get(&self_)?;
        let desc2: &Descriptor = table.get(&other)?;

        Ok(desc1.path == desc2.path)
    }

    async fn metadata_hash(
        &mut self,
        self_: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.metadata-hash");

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        match fs::metadata(&descriptor.path) {
            Ok(metadata) => {
                // Simple hash based on inode and device
                let hash = MetadataHashValue {
                    lower: metadata.ino(),
                    upper: metadata.dev(),
                };
                Ok(Ok(hash))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn metadata_hash_at(
        &mut self,
        self_: Resource<Descriptor>,
        _path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        debug!("wasi:filesystem/types/descriptor.metadata-hash-at path={}", path);

        let table = self.resource_table.lock().unwrap();
        let descriptor: &Descriptor = table.get(&self_)?;

        let full_path = descriptor.path.join(&path);

        match fs::metadata(&full_path) {
            Ok(metadata) => {
                let hash = MetadataHashValue {
                    lower: metadata.ino(),
                    upper: metadata.dev(),
                };
                Ok(Ok(hash))
            }
            Err(e) => Ok(Err(io_error_to_error_code(e))),
        }
    }

    async fn drop(&mut self, rep: Resource<Descriptor>) -> wasmtime::Result<()> {
        debug!("wasi:filesystem/types/descriptor.drop");
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// HostDirectoryEntryStream implementation
// ============================================================================

impl<E> HostDirectoryEntryStream for ActorStore<E>
where
    E: EventPayload + Clone + From<FilesystemEventData> + Send + 'static,
{
    async fn read_directory_entry(
        &mut self,
        self_: Resource<DirectoryEntryStream>,
    ) -> wasmtime::Result<Result<Option<WasiDirectoryEntry>, ErrorCode>> {
        debug!("wasi:filesystem/types/directory-entry-stream.read-directory-entry");

        let mut table = self.resource_table.lock().unwrap();
        let stream: &mut DirectoryEntryStream = table.get_mut(&self_)?;

        match stream.read_entry() {
            Some(entry) => {
                let wasi_entry = WasiDirectoryEntry {
                    type_: to_wasi_descriptor_type(entry.entry_type),
                    name: entry.name,
                };
                Ok(Ok(Some(wasi_entry)))
            }
            None => Ok(Ok(None)),
        }
    }

    async fn drop(&mut self, rep: Resource<DirectoryEntryStream>) -> wasmtime::Result<()> {
        debug!("wasi:filesystem/types/directory-entry-stream.drop");
        self.resource_table.lock().unwrap().delete(rep)?;
        Ok(())
    }
}

// ============================================================================
// FilesystemTypesHost implementation (top-level functions)
// ============================================================================

impl<E> FilesystemTypesHost for ActorStore<E>
where
    E: EventPayload + Clone + From<FilesystemEventData> + Send + 'static,
{
    async fn filesystem_error_code(
        &mut self,
        _err: Resource<IoError>,
    ) -> wasmtime::Result<Option<ErrorCode>> {
        debug!("wasi:filesystem/types.filesystem-error-code");
        Ok(None)
    }
}

// ============================================================================
// PreopensHost implementation
// ============================================================================

/// Extension type for storing preopened directories in ActorStore
/// Handlers set this during setup, and it's retrieved in get_directories
#[derive(Clone, Debug)]
pub struct FilesystemPreopens(pub Vec<(PathBuf, String)>);

impl<E> PreopensHost for ActorStore<E>
where
    E: EventPayload + Clone + From<FilesystemEventData> + Send + 'static,
{
    async fn get_directories(
        &mut self,
    ) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        debug!("wasi:filesystem/preopens.get-directories for actor {}", self.id);

        self.record_handler_event(
            "wasi:filesystem/preopens/get-directories".to_string(),
            FilesystemEventData::GetPreopensCall,
            Some("Getting preopened directories".to_string()),
        );

        // Retrieve preopens from ActorStore extensions
        let preopens = self
            .get_extension::<FilesystemPreopens>()
            .map(|p| p.0)
            .unwrap_or_default();

        debug!("Found {} preopens for actor {}", preopens.len(), self.id);

        let mut result = Vec::new();
        for (path, name) in preopens {
            let descriptor = Descriptor::new_directory(path, true);
            let resource = self.resource_table.lock().unwrap().push(descriptor)?;
            result.push((resource, name));
        }

        self.record_handler_event(
            "wasi:filesystem/preopens/get-directories".to_string(),
            FilesystemEventData::GetPreopensResult {
                count: result.len(),
            },
            Some(format!("Returned {} preopened directories", result.len())),
        );

        Ok(result)
    }
}

// ============================================================================
// Platform-specific metadata extensions
// ============================================================================

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(not(unix))]
trait MetadataExt {
    fn ino(&self) -> u64 {
        0
    }
    fn dev(&self) -> u64 {
        0
    }
    fn nlink(&self) -> u64 {
        1
    }
}

#[cfg(not(unix))]
impl MetadataExt for std::fs::Metadata {}
