use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::*;

/// Test that filesystem permission checking works correctly
#[test]
fn test_filesystem_permission_checking() {
    // Test with full permissions
    let full_permissions = Some(FileSystemPermissions {
        read: true,
        write: true,
        execute: false,
        allowed_commands: None,
        new_dir: Some(true),
        allowed_paths: Some(vec!["/tmp/test".to_string()]),
    });

    // Should allow read operation
    assert!(PermissionChecker::check_filesystem_operation(
        &full_permissions,
        "read",
        Some("/tmp/test/file.txt"),
        None,
    )
    .is_ok());

    // Should allow write operation
    assert!(PermissionChecker::check_filesystem_operation(
        &full_permissions,
        "write",
        Some("/tmp/test/file.txt"),
        None,
    )
    .is_ok());
}

#[test]
fn test_filesystem_permission_denial() {
    // Test with read-only permissions
    let read_only_permissions = Some(FileSystemPermissions {
        read: true,
        write: false,
        execute: false,
        allowed_commands: None,
        new_dir: Some(false),
        allowed_paths: Some(vec!["/tmp/test".to_string()]),
    });

    // Should allow read operation
    assert!(PermissionChecker::check_filesystem_operation(
        &read_only_permissions,
        "read",
        Some("/tmp/test/file.txt"),
        None,
    )
    .is_ok());

    // Should deny write operation
    assert!(PermissionChecker::check_filesystem_operation(
        &read_only_permissions,
        "write",
        Some("/tmp/test/file.txt"),
        None,
    )
    .is_err());
}

#[test]
fn test_filesystem_path_restrictions() {
    let restricted_permissions = Some(FileSystemPermissions {
        read: true,
        write: true,
        execute: false,
        allowed_commands: None,
        new_dir: Some(false),
        allowed_paths: Some(vec!["/tmp/allowed".to_string()]),
    });

    // Should allow operations in allowed path
    assert!(PermissionChecker::check_filesystem_operation(
        &restricted_permissions,
        "read",
        Some("/tmp/allowed/file.txt"),
        None,
    )
    .is_ok());

    // Should deny operations outside allowed path
    assert!(PermissionChecker::check_filesystem_operation(
        &restricted_permissions,
        "read",
        Some("/tmp/forbidden/file.txt"),
        None,
    )
    .is_err());
}

#[test]
fn test_filesystem_no_permissions() {
    // No permissions granted
    let no_permissions: Option<FileSystemPermissions> = None;

    // Should deny all operations when no permissions are granted
    assert!(PermissionChecker::check_filesystem_operation(
        &no_permissions,
        "read",
        Some("/tmp/test/file.txt"),
        None,
    )
    .is_err());
}

#[test]
fn test_http_permission_checking() {
    let http_permissions = Some(HttpClientPermissions {
        allowed_methods: Some(vec!["GET".to_string(), "POST".to_string()]),
        allowed_hosts: Some(vec!["api.example.com".to_string()]),
        max_redirects: Some(10),
        timeout: Some(30000),
    });

    // Should allow GET request to allowed host
    assert!(PermissionChecker::check_http_operation(
        &http_permissions,
        "GET",
        "api.example.com",
    )
    .is_ok());

    // Should allow POST request to allowed host
    assert!(PermissionChecker::check_http_operation(
        &http_permissions,
        "POST",
        "api.example.com",
    )
    .is_ok());

    // Should deny PUT request (not in allowed methods)
    assert!(PermissionChecker::check_http_operation(
        &http_permissions,
        "PUT",
        "api.example.com",
    )
    .is_err());

    // Should deny request to forbidden host
    assert!(PermissionChecker::check_http_operation(
        &http_permissions,
        "GET",
        "forbidden.com",
    )
    .is_err());
}

#[test]
fn test_environment_permission_checking() {
    let env_permissions = Some(EnvironmentPermissions {
        allowed_vars: Some(vec!["HOME".to_string(), "PATH".to_string()]),
        denied_vars: None,
        allow_list_all: false,
        allowed_prefixes: None,
    });

    // Should allow reading allowed environment variables
    assert!(PermissionChecker::check_env_var_access(
        &env_permissions,
        "HOME",
    )
    .is_ok());

    assert!(PermissionChecker::check_env_var_access(
        &env_permissions,
        "PATH",
    )
    .is_ok());

    // Should deny reading forbidden environment variables
    assert!(PermissionChecker::check_env_var_access(
        &env_permissions,
        "SECRET_KEY",
    )
    .is_err());
}

#[test]
fn test_environment_wildcard_permissions() {
    let wildcard_permissions = Some(EnvironmentPermissions {
        allowed_vars: None,
        denied_vars: None,
        allow_list_all: true, // Allow all variables
        allowed_prefixes: None,
    });

    // Should allow access to any environment variable with allow_list_all
    assert!(PermissionChecker::check_env_var_access(
        &wildcard_permissions,
        "ANY_VARIABLE",
    )
    .is_ok());

    assert!(PermissionChecker::check_env_var_access(
        &wildcard_permissions,
        "ANOTHER_VARIABLE",
    )
    .is_ok());
}

#[test]
fn test_process_permission_checking() {
    let process_permissions = Some(ProcessPermissions {
        allowed_programs: Some(vec!["ls".to_string(), "cat".to_string()]),
        max_processes: 5,
        max_output_buffer: 1024,
        allowed_paths: None,
    });

    // Should allow running allowed programs
    assert!(PermissionChecker::check_process_operation(
        &process_permissions,
        "ls",
        0, // current process count
    )
    .is_ok());

    assert!(PermissionChecker::check_process_operation(
        &process_permissions,
        "cat",
        0, // current process count
    )
    .is_ok());

    // Should deny running forbidden programs
    assert!(PermissionChecker::check_process_operation(
        &process_permissions,
        "rm",
        0, // current process count
    )
    .is_err());

    // Should deny when process limit exceeded
    assert!(PermissionChecker::check_process_operation(
        &process_permissions,
        "ls",
        5, // current process count at limit
    )
    .is_err());
}

#[test]
fn test_random_permission_checking() {
    let random_permissions = Some(RandomPermissions {
        max_bytes: 1024,
        max_int: 100,
        allow_crypto_secure: false,
    });

    // Should allow requests within limits
    assert!(PermissionChecker::check_random_operation(
        &random_permissions,
        "bytes",
        Some(512),
        None,
    )
    .is_ok());

    assert!(PermissionChecker::check_random_operation(
        &random_permissions,
        "range",
        None,
        Some(50),
    )
    .is_ok());

    // Should deny requests exceeding limits
    assert!(PermissionChecker::check_random_operation(
        &random_permissions,
        "bytes",
        Some(2048),
        None,
    )
    .is_err());

    assert!(PermissionChecker::check_random_operation(
        &random_permissions,
        "range",
        None,
        Some(200),
    )
    .is_err());
}

#[test]
fn test_timing_permission_checking() {
    let timing_permissions = Some(TimingPermissions {
        max_sleep_duration: 5000,
    });

    // Should allow sleep within limits
    assert!(PermissionChecker::check_timing_operation(
        &timing_permissions,
        "sleep",
        3000,
    )
    .is_ok());

    // Should deny sleep exceeding limits
    assert!(PermissionChecker::check_timing_operation(
        &timing_permissions,
        "sleep",
        10000,
    )
    .is_err());
}
