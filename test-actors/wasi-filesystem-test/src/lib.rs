#![allow(warnings)]

mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::wasi::filesystem::preopens::get_directories;
use bindings::wasi::filesystem::types::{
    Descriptor, DescriptorFlags, DescriptorType, OpenFlags, PathFlags,
};

struct WasiFilesystemTest;

impl Guest for WasiFilesystemTest {
    fn init(_state: Option<Vec<u8>>, _params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        let mut results = Vec::new();
        results.push("Starting WASI filesystem tests...".to_string());

        // Test 1: Get preopened directories
        results.push("\n=== Test 1: Get preopened directories ===".to_string());
        let preopens = get_directories();
        results.push(format!("Found {} preopened directories", preopens.len()));

        if preopens.is_empty() {
            return Err("No preopened directories found!".to_string());
        }

        for (descriptor, path) in &preopens {
            results.push(format!("  Preopen: '{}'", path));

            // Get the type of the descriptor
            match descriptor.get_type() {
                Ok(dtype) => {
                    let type_str = match dtype {
                        DescriptorType::Directory => "directory",
                        DescriptorType::RegularFile => "file",
                        DescriptorType::SymbolicLink => "symlink",
                        _ => "other",
                    };
                    results.push(format!("    Type: {}", type_str));
                }
                Err(e) => {
                    results.push(format!("    Failed to get type: {:?}", e));
                }
            }
        }

        // Use the first preopened directory for tests
        let (root_dir, root_path) = &preopens[0];

        // Test 2: Read directory contents
        results.push("\n=== Test 2: Read directory contents ===".to_string());
        match root_dir.read_directory() {
            Ok(dir_stream) => {
                let mut entry_count = 0;
                loop {
                    match dir_stream.read_directory_entry() {
                        Ok(Some(entry)) => {
                            let type_str = match entry.type_ {
                                DescriptorType::Directory => "dir",
                                DescriptorType::RegularFile => "file",
                                DescriptorType::SymbolicLink => "symlink",
                                _ => "other",
                            };
                            results.push(format!("  [{}] {}", type_str, entry.name));
                            entry_count += 1;
                            if entry_count >= 10 {
                                results.push("  ... (truncated)".to_string());
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            results.push(format!("  Error reading entry: {:?}", e));
                            break;
                        }
                    }
                }
                results.push(format!("Listed {} entries", entry_count));
            }
            Err(e) => {
                results.push(format!("Failed to read directory: {:?}", e));
            }
        }

        // Test 3: Create a test directory
        results.push("\n=== Test 3: Create directory ===".to_string());
        let test_dir_name = "wasi-fs-test-dir";
        match root_dir.create_directory_at(test_dir_name) {
            Ok(()) => {
                results.push(format!("Created directory: {}", test_dir_name));
            }
            Err(e) => {
                // Directory might already exist
                results.push(format!("Create directory result: {:?} (may already exist)", e));
            }
        }

        // Test 4: Open the test directory
        results.push("\n=== Test 4: Open directory ===".to_string());
        let open_flags = OpenFlags::DIRECTORY;
        let desc_flags = DescriptorFlags::READ;
        match root_dir.open_at(PathFlags::empty(), test_dir_name, open_flags, desc_flags) {
            Ok(test_dir) => {
                results.push(format!("Opened directory: {}", test_dir_name));

                // Get stat on the directory
                match test_dir.stat() {
                    Ok(stat) => {
                        results.push(format!("  Size: {} bytes", stat.size));
                        results.push(format!("  Link count: {}", stat.link_count));
                    }
                    Err(e) => {
                        results.push(format!("  Failed to stat: {:?}", e));
                    }
                }
            }
            Err(e) => {
                results.push(format!("Failed to open directory: {:?}", e));
            }
        }

        // Test 5: Create and write to a file
        results.push("\n=== Test 5: Create and write file ===".to_string());
        let test_file_name = "wasi-fs-test-file.txt";
        let open_flags = OpenFlags::CREATE;
        let desc_flags = DescriptorFlags::READ | DescriptorFlags::WRITE;

        match root_dir.open_at(PathFlags::empty(), test_file_name, open_flags, desc_flags) {
            Ok(file) => {
                results.push(format!("Opened/created file: {}", test_file_name));

                // Write some data
                let test_data = b"Hello from WASI filesystem test!";
                match file.write(test_data, 0) {
                    Ok(bytes_written) => {
                        results.push(format!("Wrote {} bytes", bytes_written));
                    }
                    Err(e) => {
                        results.push(format!("Failed to write: {:?}", e));
                    }
                }

                // Sync the file
                match file.sync() {
                    Ok(()) => results.push("File synced".to_string()),
                    Err(e) => results.push(format!("Sync failed: {:?}", e)),
                }
            }
            Err(e) => {
                results.push(format!("Failed to create file: {:?}", e));
            }
        }

        // Test 6: Read the file back
        results.push("\n=== Test 6: Read file ===".to_string());
        let open_flags = OpenFlags::empty();
        let desc_flags = DescriptorFlags::READ;

        match root_dir.open_at(PathFlags::empty(), test_file_name, open_flags, desc_flags) {
            Ok(file) => {
                match file.read(100, 0) {
                    Ok((data, at_end)) => {
                        let content = String::from_utf8_lossy(&data);
                        results.push(format!("Read {} bytes: '{}'", data.len(), content));
                        results.push(format!("At end: {}", at_end));
                    }
                    Err(e) => {
                        results.push(format!("Failed to read: {:?}", e));
                    }
                }
            }
            Err(e) => {
                results.push(format!("Failed to open file for reading: {:?}", e));
            }
        }

        // Test 7: Get file stats
        results.push("\n=== Test 7: Stat file ===".to_string());
        match root_dir.stat_at(PathFlags::empty(), test_file_name) {
            Ok(stat) => {
                let type_str = match stat.type_ {
                    DescriptorType::RegularFile => "file",
                    DescriptorType::Directory => "directory",
                    _ => "other",
                };
                results.push(format!("File type: {}", type_str));
                results.push(format!("File size: {} bytes", stat.size));
            }
            Err(e) => {
                results.push(format!("Failed to stat: {:?}", e));
            }
        }

        // Test 8: Clean up - delete the test file
        results.push("\n=== Test 8: Clean up ===".to_string());
        match root_dir.unlink_file_at(test_file_name) {
            Ok(()) => results.push(format!("Deleted file: {}", test_file_name)),
            Err(e) => results.push(format!("Failed to delete file: {:?}", e)),
        }

        match root_dir.remove_directory_at(test_dir_name) {
            Ok(()) => results.push(format!("Deleted directory: {}", test_dir_name)),
            Err(e) => results.push(format!("Failed to delete directory: {:?}", e)),
        }

        results.push("\n=== WASI filesystem tests completed! ===".to_string());

        Ok((Some(results.join("\n").into_bytes()),))
    }
}

bindings::export!(WasiFilesystemTest with_types_in bindings);
