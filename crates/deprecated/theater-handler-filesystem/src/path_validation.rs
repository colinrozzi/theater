//! Path validation and resolution for filesystem operations

use std::path::{Path, PathBuf};
use theater::config::permissions::FileSystemPermissions;

/// Resolve and validate a path against permissions
///
/// This function:
/// 1. For creation operations (write, create-dir): validates the parent directory exists and is allowed
/// 2. For other operations (read, delete, list, etc.): validates the target path exists and is allowed
/// 3. Resolves the path (handles ., .., etc.)
/// 4. Checks if the resolved path is within allowed paths
///
/// Returns the resolved path that should be used for the operation
pub fn resolve_and_validate_path(
    base_path: &Path,
    requested_path: &str,
    operation: &str,
    permissions: &Option<FileSystemPermissions>,
) -> Result<PathBuf, String> {
    // 1. Append requested path to base path
    let full_path = base_path.join(requested_path);

    // 2. Determine if this is a creation operation
    let is_creation = matches!(operation, "write" | "create-dir");

    // 3. For creation operations, validate the parent directory
    //    For other operations, validate the target path
    let path_to_validate = if is_creation {
        // For creation, we need to validate the parent directory
        full_path.parent().ok_or_else(|| {
            "Cannot determine parent directory for creation operation".to_string()
        })?
    } else {
        // For read/delete operations, validate the target path
        &full_path
    };

    // 4. Use dunce for robust path canonicalization
    let resolved_validation_path = dunce::canonicalize(path_to_validate).map_err(|e| {
        if is_creation {
            format!(
                "Failed to resolve parent directory '{}' for creation operation: {}",
                path_to_validate.display(),
                e
            )
        } else {
            format!(
                "Failed to resolve path '{}': {}",
                path_to_validate.display(),
                e
            )
        }
    })?;

    // 5. Check if resolved path is within allowed paths
    if let Some(perms) = permissions {
        if let Some(allowed_paths) = &perms.allowed_paths {
            let is_allowed = allowed_paths.iter().any(|allowed_path| {
                // Canonicalize the allowed path for comparison using dunce
                let allowed_canonical = dunce::canonicalize(allowed_path)
                    .unwrap_or_else(|_| PathBuf::from(allowed_path));

                // Check if resolved path is within the allowed directory
                resolved_validation_path == allowed_canonical
                    || resolved_validation_path.starts_with(&allowed_canonical)
            });

            if !is_allowed {
                return Err(if is_creation {
                    format!(
                        "Parent directory '{}' not in allowed paths for creation operation: {:?}",
                        resolved_validation_path.display(),
                        allowed_paths
                    )
                } else {
                    format!(
                        "Path '{}' not in allowed paths: {:?}",
                        resolved_validation_path.display(),
                        allowed_paths
                    )
                });
            }
        }
    }

    // 6. For creation operations, construct the final path from canonicalized parent + filename
    //    For other operations, return the canonicalized path
    if is_creation {
        // For creation, we've validated the parent, now construct the target path
        // by appending the filename/dirname to the canonicalized parent directory
        let final_component = full_path.file_name().ok_or_else(|| {
            format!(
                "Cannot determine target name for {} operation on path '{}'",
                operation, requested_path
            )
        })?;

        Ok(resolved_validation_path.join(final_component))
    } else {
        // For read/delete, return the canonicalized path
        Ok(dunce::canonicalize(&full_path).map_err(|e| {
            format!(
                "Failed to resolve target path '{}': {}",
                full_path.display(),
                e
            )
        })?)
    }
}
