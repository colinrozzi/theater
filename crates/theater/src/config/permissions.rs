//! Permission hierarchy with “parent ≥ child” semantics.
//!
//! **Rule of thumb**
//! * `None` in a **parent** ⇒ capability is **denied** → child must also be `None`.
//! * `None` in a **child**  ⇒ it requests nothing extra → always allowed.
//!
//! For every permission struct `PartialOrd` is implemented such that
//! `parent >= child` **iff** the child does not exceed the authority of the parent.
//!
//! The top-level `HandlerPermission` aggregates all capability types and enforces the
//! rule field-by-field.

use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet, hash::Hash};

/// Trait for applying restrictions to permission types
pub trait RestrictWith<T> {
    fn restrict_with(&self, restriction: &T) -> Self;
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Helper functions                                                          */
/* ─────────────────────────────────────────────────────────────────────────── */

/// Returns `true` when `candidate` is *totally* covered by `parent`.
pub fn permits(parent: &HandlerPermission, candidate: &HandlerPermission) -> bool {
    matches!(
        parent.partial_cmp(candidate),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    )
}

/// Generic helper: returns `true` when the **child** request is allowed under
/// the **parent** capability, according to the comparator `cmp` on concrete
/// values.
///
/// * `parent == None` and `child == Some(_)` → **false** (capability denied).
/// * `child  == None`                        → **true**  (child silent).
fn option_subset<P, F>(parent: &Option<P>, child: &Option<P>, cmp: F) -> bool
where
    F: Fn(&P, &P) -> bool,
{
    match (parent, child) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(p), Some(c)) => cmp(p, c),
    }
}

/// Convenience: is `parent` a **superset** of `child` for `Vec`s?
fn vec_superset<T: Eq + Hash>(parent: &Vec<T>, child: &Vec<T>) -> bool {
    parent
        .iter()
        .collect::<HashSet<_>>()
        .is_superset(&child.iter().collect::<HashSet<_>>())
}

/// Convenience: numeric ≤ (child ≤ parent).
fn le_num<T: PartialOrd>(parent: &T, child: &T) -> bool {
    child <= parent
}

/// Helper: intersection of two optional vectors
fn intersect_options<T: Clone + Eq + std::hash::Hash>(
    first: &Option<Vec<T>>,
    second: &Option<Vec<T>>,
) -> Option<Vec<T>> {
    match (first, second) {
        (Some(first_list), Some(second_list)) => {
            let first_set: HashSet<_> = first_list.iter().collect();
            let intersection: Vec<T> = second_list
                .iter()
                .filter(|item| first_set.contains(item))
                .cloned()
                .collect();
            Some(intersection)
        }
        (Some(first_list), None) => Some(first_list.clone()),
        (None, Some(_)) => None, // First denies capability
        (None, None) => None,
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  File-system permissions                                                   */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSystemPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub allowed_commands: Option<Vec<String>>, // ⊇
    pub new_dir: Option<bool>,                 // ≥
    pub allowed_paths: Option<Vec<String>>,    // ⊇
}

impl PartialOrd for FileSystemPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if (self.read < other.read) || (self.write < other.write) || (self.execute < other.execute)
        {
            return None;
        }
        if !option_subset(&self.new_dir, &other.new_dir, |p, c| p >= c) {
            return None;
        }
        if !option_subset(
            &self.allowed_commands,
            &other.allowed_commands,
            vec_superset,
        ) {
            return None;
        }
        if !option_subset(&self.allowed_paths, &other.allowed_paths, vec_superset) {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  HTTP-client permissions                                                   */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpClientPermissions {
    pub allowed_methods: Option<Vec<String>>, // subset
    pub allowed_hosts: Option<Vec<String>>,   // subset
    pub max_redirects: Option<usize>,         // ≤
    pub timeout: Option<u64>,                 // ≤ (milliseconds)
}

impl PartialOrd for HttpClientPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if !option_subset(&self.allowed_methods, &other.allowed_methods, vec_superset) {
            return None;
        }
        if !option_subset(&self.allowed_hosts, &other.allowed_hosts, vec_superset) {
            return None;
        }
        if !option_subset(&self.max_redirects, &other.max_redirects, le_num) {
            return None;
        }
        if !option_subset(&self.timeout, &other.timeout, le_num) {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  HTTP-framework permissions                                               */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpFrameworkPermissions {
    pub allowed_routes: Option<Vec<String>>,  // subset
    pub allowed_methods: Option<Vec<String>>, // subset
    pub max_request_size: Option<usize>,      // ≤ (bytes)
}

impl PartialOrd for HttpFrameworkPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if !option_subset(&self.allowed_routes, &other.allowed_routes, vec_superset) {
            return None;
        }
        if !option_subset(&self.allowed_methods, &other.allowed_methods, vec_superset) {
            return None;
        }
        if !option_subset(&self.max_request_size, &other.max_request_size, le_num) {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Process permissions                                                       */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessPermissions {
    pub max_processes: usize,                  // ≥
    pub max_output_buffer: usize,              // ≥
    pub allowed_programs: Option<Vec<String>>, // subset
    pub allowed_paths: Option<Vec<String>>,    // subset
}

impl PartialOrd for ProcessPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.max_processes < other.max_processes {
            return None;
        }
        if self.max_output_buffer < other.max_output_buffer {
            return None;
        }
        if !option_subset(
            &self.allowed_programs,
            &other.allowed_programs,
            vec_superset,
        ) {
            return None;
        }
        if !option_subset(&self.allowed_paths, &other.allowed_paths, vec_superset) {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Environment permissions                                                   */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentPermissions {
    pub allowed_vars: Option<Vec<String>>,     // subset
    pub denied_vars: Option<Vec<String>>,      // subset (parent ⊇ child)
    pub allow_list_all: bool,                  // ≥ (true is more permissive)
    pub allowed_prefixes: Option<Vec<String>>, // subset
}

impl PartialOrd for EnvironmentPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if !option_subset(&self.allowed_vars, &other.allowed_vars, vec_superset) {
            return None;
        }
        if !option_subset(&self.denied_vars, &other.denied_vars, vec_superset) {
            return None;
        }
        if self.allow_list_all < other.allow_list_all {
            return None;
        }
        if !option_subset(
            &self.allowed_prefixes,
            &other.allowed_prefixes,
            vec_superset,
        ) {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Random permissions                                                        */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RandomPermissions {
    pub max_bytes: usize,          // ≥ (parent must allow at least as many)
    pub max_int: u64,              // ≥
    pub allow_crypto_secure: bool, // ≥
}

impl PartialOrd for RandomPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.max_bytes < other.max_bytes {
            return None;
        }
        if self.max_int < other.max_int {
            return None;
        }
        if self.allow_crypto_secure < other.allow_crypto_secure {
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Timing permissions                                                        */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimingPermissions {
    pub max_sleep_duration: u64, // ms – greater value = more freedom
    pub min_sleep_duration: u64, // ms – smaller value = more freedom
}

impl PartialOrd for TimingPermissions {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.max_sleep_duration < other.max_sleep_duration {
            return None;
        }
        if self.min_sleep_duration > other.min_sleep_duration {
            // parent enforces a higher minimum → child cannot go below it
            return None;
        }
        Some(if self == other {
            Ordering::Equal
        } else {
            Ordering::Greater
        })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Simple capability types that are always “equal”                           */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageServerPermissions;
impl PartialOrd for MessageServerPermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePermissions;
impl PartialOrd for RuntimePermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorPermissions;
impl PartialOrd for SupervisorPermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorePermissions;
impl PartialOrd for StorePermissions {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  Top-level aggregation                                                     */
/* ─────────────────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HandlerPermission {
    pub message_server: Option<MessageServerPermissions>,
    pub file_system: Option<FileSystemPermissions>,
    pub http_client: Option<HttpClientPermissions>,
    pub http_framework: Option<HttpFrameworkPermissions>,
    pub runtime: Option<RuntimePermissions>,
    pub supervisor: Option<SupervisorPermissions>,
    pub store: Option<StorePermissions>,
    pub timing: Option<TimingPermissions>,
    pub process: Option<ProcessPermissions>,
    pub environment: Option<EnvironmentPermissions>,
    pub random: Option<RandomPermissions>,
}

impl HandlerPermission {
    /// Create a root permission set that allows all capabilities
    pub fn root() -> Self {
        HandlerPermission {
            message_server: Some(MessageServerPermissions),
            file_system: Some(FileSystemPermissions {
                read: true,
                write: true,
                execute: true,
                allowed_commands: None, // None means all commands allowed
                new_dir: Some(true),
                allowed_paths: Some(vec!["/".to_string()]), // Root can access all paths
            }),
            http_client: Some(HttpClientPermissions {
                allowed_methods: None, // None means all methods allowed
                allowed_hosts: None, // None means all hosts allowed
                max_redirects: None, // None means unlimited
                timeout: None, // None means no timeout restriction
            }),
            http_framework: Some(HttpFrameworkPermissions {
                allowed_routes: None,
                allowed_methods: None,
                max_request_size: None,
            }),
            runtime: Some(RuntimePermissions),
            supervisor: Some(SupervisorPermissions),
            store: Some(StorePermissions),
            timing: Some(TimingPermissions {
                max_sleep_duration: u64::MAX,
                min_sleep_duration: 0,
            }),
            process: Some(ProcessPermissions {
                max_processes: usize::MAX,
                max_output_buffer: usize::MAX,
                allowed_programs: None,
                allowed_paths: None,
            }),
            environment: Some(EnvironmentPermissions {
                allowed_vars: None,
                denied_vars: None,
                allow_list_all: true,
                allowed_prefixes: None,
            }),
            random: Some(RandomPermissions {
                max_bytes: usize::MAX,
                max_int: u64::MAX,
                allow_crypto_secure: true,
            }),
        }
    }

    /// Calculate effective permissions from parent permissions and inheritance policy
    pub fn calculate_effective(
        parent_permissions: &HandlerPermission,
        policy: &crate::config::inheritance::HandlerPermissionPolicy,
    ) -> HandlerPermission {
        use crate::config::inheritance::apply_inheritance_policy;
        
        HandlerPermission {
            message_server: apply_inheritance_policy(
                &parent_permissions.message_server,
                &policy.message_server,
            ),
            file_system: apply_inheritance_policy(
                &parent_permissions.file_system,
                &policy.file_system,
            ),
            http_client: apply_inheritance_policy(
                &parent_permissions.http_client,
                &policy.http_client,
            ),
            http_framework: apply_inheritance_policy(
                &parent_permissions.http_framework,
                &policy.http_framework,
            ),
            runtime: apply_inheritance_policy(
                &parent_permissions.runtime,
                &policy.runtime,
            ),
            supervisor: apply_inheritance_policy(
                &parent_permissions.supervisor,
                &policy.supervisor,
            ),
            store: apply_inheritance_policy(
                &parent_permissions.store,
                &policy.store,
            ),
            timing: apply_inheritance_policy(
                &parent_permissions.timing,
                &policy.timing,
            ),
            process: apply_inheritance_policy(
                &parent_permissions.process,
                &policy.process,
            ),
            environment: apply_inheritance_policy(
                &parent_permissions.environment,
                &policy.environment,
            ),
            random: apply_inheritance_policy(
                &parent_permissions.random,
                &policy.random,
            ),
        }
    }
}

impl PartialOrd for HandlerPermission {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use Ordering::*;
        // Check every field with `option_subset` and capability-specific `>=`.
        let checks = [
            option_subset(&self.message_server, &other.message_server, |p, c| p >= c),
            option_subset(&self.file_system, &other.file_system, |p, c| p >= c),
            option_subset(&self.http_client, &other.http_client, |p, c| p >= c),
            option_subset(&self.http_framework, &other.http_framework, |p, c| p >= c),
            option_subset(&self.runtime, &other.runtime, |p, c| p >= c),
            option_subset(&self.supervisor, &other.supervisor, |p, c| p >= c),
            option_subset(&self.store, &other.store, |p, c| p >= c),
            option_subset(&self.timing, &other.timing, |p, c| p >= c),
            option_subset(&self.process, &other.process, |p, c| p >= c),
            option_subset(&self.environment, &other.environment, |p, c| p >= c),
            option_subset(&self.random, &other.random, |p, c| p >= c),
        ];
        if !checks.iter().all(|&ok| ok) {
            return None;
        }
        let strictly = [
            self.message_server != other.message_server,
            self.file_system != other.file_system,
            self.http_client != other.http_client,
            self.http_framework != other.http_framework,
            self.runtime != other.runtime,
            self.supervisor != other.supervisor,
            self.store != other.store,
            self.timing != other.timing,
            self.process != other.process,
            self.environment != other.environment,
            self.random != other.random,
        ]
        .iter()
        .any(|&d| d);
        Some(if strictly { Greater } else { Equal })
    }
}

/* ─────────────────────────────────────────────────────────────────────────── */
/*  RestrictWith implementations                                               */
/* ─────────────────────────────────────────────────────────────────────────── */

impl RestrictWith<FileSystemPermissions> for FileSystemPermissions {
    fn restrict_with(&self, restriction: &FileSystemPermissions) -> Self {
        FileSystemPermissions {
            read: self.read && restriction.read,
            write: self.write && restriction.write,
            execute: self.execute && restriction.execute,
            allowed_commands: intersect_options(&self.allowed_commands, &restriction.allowed_commands),
            new_dir: self.new_dir.and_then(|p| restriction.new_dir.map(|r| p && r)),
            allowed_paths: intersect_options(&self.allowed_paths, &restriction.allowed_paths),
        }
    }
}

impl RestrictWith<HttpClientPermissions> for HttpClientPermissions {
    fn restrict_with(&self, restriction: &HttpClientPermissions) -> Self {
        HttpClientPermissions {
            allowed_methods: intersect_options(&self.allowed_methods, &restriction.allowed_methods),
            allowed_hosts: intersect_options(&self.allowed_hosts, &restriction.allowed_hosts),
            max_redirects: match (self.max_redirects, restriction.max_redirects) {
                (Some(parent), Some(restrict)) => Some(parent.min(restrict)),
                (Some(parent), None) => Some(parent),
                (None, _) => None,
            },
            timeout: match (self.timeout, restriction.timeout) {
                (Some(parent), Some(restrict)) => Some(parent.min(restrict)),
                (Some(parent), None) => Some(parent),
                (None, _) => None,
            },
        }
    }
}

impl RestrictWith<HttpFrameworkPermissions> for HttpFrameworkPermissions {
    fn restrict_with(&self, restriction: &HttpFrameworkPermissions) -> Self {
        HttpFrameworkPermissions {
            allowed_routes: intersect_options(&self.allowed_routes, &restriction.allowed_routes),
            allowed_methods: intersect_options(&self.allowed_methods, &restriction.allowed_methods),
            max_request_size: match (self.max_request_size, restriction.max_request_size) {
                (Some(parent), Some(restrict)) => Some(parent.min(restrict)),
                (Some(parent), None) => Some(parent),
                (None, _) => None,
            },
        }
    }
}

impl RestrictWith<ProcessPermissions> for ProcessPermissions {
    fn restrict_with(&self, restriction: &ProcessPermissions) -> Self {
        ProcessPermissions {
            max_processes: self.max_processes.min(restriction.max_processes),
            max_output_buffer: self.max_output_buffer.min(restriction.max_output_buffer),
            allowed_programs: intersect_options(&self.allowed_programs, &restriction.allowed_programs),
            allowed_paths: intersect_options(&self.allowed_paths, &restriction.allowed_paths),
        }
    }
}

impl RestrictWith<EnvironmentPermissions> for EnvironmentPermissions {
    fn restrict_with(&self, restriction: &EnvironmentPermissions) -> Self {
        EnvironmentPermissions {
            allowed_vars: intersect_options(&self.allowed_vars, &restriction.allowed_vars),
            denied_vars: match (&self.denied_vars, &restriction.denied_vars) {
                (Some(parent), Some(restrict)) => {
                    let mut combined = parent.clone();
                    combined.extend_from_slice(restrict);
                    combined.dedup();
                    Some(combined)
                },
                (Some(parent), None) => Some(parent.clone()),
                (None, Some(restrict)) => Some(restrict.clone()),
                (None, None) => None,
            },
            allow_list_all: self.allow_list_all && restriction.allow_list_all,
            allowed_prefixes: intersect_options(&self.allowed_prefixes, &restriction.allowed_prefixes),
        }
    }
}

impl RestrictWith<RandomPermissions> for RandomPermissions {
    fn restrict_with(&self, restriction: &RandomPermissions) -> Self {
        RandomPermissions {
            max_bytes: self.max_bytes.min(restriction.max_bytes),
            max_int: self.max_int.min(restriction.max_int),
            allow_crypto_secure: self.allow_crypto_secure && restriction.allow_crypto_secure,
        }
    }
}

impl RestrictWith<TimingPermissions> for TimingPermissions {
    fn restrict_with(&self, restriction: &TimingPermissions) -> Self {
        TimingPermissions {
            max_sleep_duration: self.max_sleep_duration.min(restriction.max_sleep_duration),
            min_sleep_duration: self.min_sleep_duration.max(restriction.min_sleep_duration),
        }
    }
}

// Simple permissions that don't restrict
impl RestrictWith<MessageServerPermissions> for MessageServerPermissions {
    fn restrict_with(&self, _restriction: &MessageServerPermissions) -> Self {
        self.clone()
    }
}

impl RestrictWith<RuntimePermissions> for RuntimePermissions {
    fn restrict_with(&self, _restriction: &RuntimePermissions) -> Self {
        self.clone()
    }
}

impl RestrictWith<SupervisorPermissions> for SupervisorPermissions {
    fn restrict_with(&self, _restriction: &SupervisorPermissions) -> Self {
        self.clone()
    }
}

impl RestrictWith<StorePermissions> for StorePermissions {
    fn restrict_with(&self, _restriction: &StorePermissions) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    // Helper function to create a full-capability filesystem permission
    fn full_filesystem_permissions() -> FileSystemPermissions {
        FileSystemPermissions {
            read: true,
            write: true,
            execute: true,
            allowed_commands: Some(vec!["ls".to_string(), "cat".to_string(), "echo".to_string()]),
            new_dir: Some(true),
            allowed_paths: Some(vec!["/home".to_string(), "/tmp".to_string(), "/data".to_string()]),
        }
    }

    #[test]
    fn test_filesystem_restrict_with_boolean_flags() {
        let parent = full_filesystem_permissions();
        let restriction = FileSystemPermissions {
            read: false,
            write: true,
            execute: false,
            allowed_commands: None,
            new_dir: None,
            allowed_paths: None,
        };

        let result = parent.restrict_with(&restriction);

        assert!(!result.read);
        assert!(result.write);
        assert!(!result.execute);
    }

    #[test]
    fn test_security_property_child_cannot_exceed_parent() {
        let parent = FileSystemPermissions {
            read: true,
            write: false,
            execute: true,
            allowed_commands: Some(vec!["ls".to_string()]),
            new_dir: Some(false),
            allowed_paths: Some(vec!["/home".to_string()]),
        };

        let greedy_restriction = FileSystemPermissions {
            read: true,
            write: true, // Child wants write but parent doesn't have it
            execute: true,
            allowed_commands: Some(vec!["ls".to_string(), "rm".to_string()]),
            new_dir: Some(true),
            allowed_paths: Some(vec!["/home".to_string(), "/root".to_string()]),
        };

        let result = parent.restrict_with(&greedy_restriction);

        assert!(result.read);
        assert!(!result.write); // Parent doesn't have it
        assert!(result.execute);
        assert_eq!(result.allowed_commands, Some(vec!["ls".to_string()]));
        assert_eq!(result.new_dir, Some(false));
        assert_eq!(result.allowed_paths, Some(vec!["/home".to_string()]));
    }

    #[test]
    fn test_root_permissions_comprehensive() {
        let root = HandlerPermission::root();

        assert!(root.file_system.is_some());
        assert!(root.http_client.is_some());
        assert!(root.process.is_some());

        let fs = root.file_system.unwrap();
        assert!(fs.read && fs.write && fs.execute);
        assert_eq!(fs.allowed_commands, None);
        assert_eq!(fs.allowed_paths, None);
    }
}
