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
