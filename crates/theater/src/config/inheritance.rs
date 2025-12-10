use super::permissions::RestrictWith;
use serde::{Deserialize, Serialize};

/// Defines how a handler should inherit permissions from its parent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum HandlerInheritance<T> {
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "disallow")]
    Disallow,
    #[serde(rename = "restrict")]
    Restrict(T),
}

impl<T> Default for HandlerInheritance<T> {
    fn default() -> Self {
        Self::Inherit
    }
}

impl<T> PartialEq for HandlerInheritance<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (HandlerInheritance::Inherit, HandlerInheritance::Inherit) => true,
            (HandlerInheritance::Disallow, HandlerInheritance::Disallow) => true,
            (HandlerInheritance::Restrict(a), HandlerInheritance::Restrict(b)) => a == b,
            _ => false,
        }
    }
}

// Helper functions for skip_serializing_if
fn is_inherit_message_server(
    val: &HandlerInheritance<crate::config::permissions::MessageServerPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_file_system(
    val: &HandlerInheritance<crate::config::permissions::FileSystemPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_http_client(
    val: &HandlerInheritance<crate::config::permissions::HttpClientPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_http_framework(
    val: &HandlerInheritance<crate::config::permissions::HttpFrameworkPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_runtime(
    val: &HandlerInheritance<crate::config::permissions::RuntimePermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_supervisor(
    val: &HandlerInheritance<crate::config::permissions::SupervisorPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_store(
    val: &HandlerInheritance<crate::config::permissions::StorePermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_timing(
    val: &HandlerInheritance<crate::config::permissions::TimingPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_process(
    val: &HandlerInheritance<crate::config::permissions::ProcessPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_environment(
    val: &HandlerInheritance<crate::config::permissions::EnvironmentPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

fn is_inherit_random(
    val: &HandlerInheritance<crate::config::permissions::RandomPermissions>,
) -> bool {
    matches!(val, HandlerInheritance::Inherit)
}

/// Helper function to check if the entire HandlerPermissionPolicy is at its default value
pub fn is_default_permission_policy(policy: &HandlerPermissionPolicy) -> bool {
    policy == &HandlerPermissionPolicy::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::permissions::*;

    #[test]
    fn test_inheritance_policy_comprehensive() {
        let parent_perms = HandlerPermission {
            file_system: Some(FileSystemPermissions {
                read: true,
                write: true,
                execute: true,
                allowed_commands: Some(vec!["ls".to_string(), "cat".to_string()]),
                new_dir: Some(true),
                allowed_paths: Some(vec!["/home".to_string()]),
            }),
            ..Default::default()
        };

        let policy = HandlerPermissionPolicy {
            file_system: HandlerInheritance::Restrict(FileSystemPermissions {
                read: true,
                write: false,
                execute: true,
                allowed_commands: Some(vec!["ls".to_string()]),
                new_dir: Some(false),
                allowed_paths: None,
            }),
            ..Default::default()
        };

        let result = HandlerPermission::calculate_effective(&parent_perms, &policy);
        let fs = result.file_system.unwrap();
        assert!(fs.read);
        assert!(!fs.write);
        assert_eq!(fs.allowed_commands, Some(vec!["ls".to_string()]));
    }

    #[test]
    fn test_default_inheritance_is_inherit() {
        let policy = HandlerPermissionPolicy::default();
        assert!(matches!(policy.file_system, HandlerInheritance::Inherit));
        assert!(matches!(policy.http_client, HandlerInheritance::Inherit));
        assert!(matches!(policy.process, HandlerInheritance::Inherit));
    }
}

/// Per-handler permission inheritance policies
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HandlerPermissionPolicy {
    #[serde(default, skip_serializing_if = "is_inherit_message_server")]
    pub message_server: HandlerInheritance<crate::config::permissions::MessageServerPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_file_system")]
    pub file_system: HandlerInheritance<crate::config::permissions::FileSystemPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_http_client")]
    pub http_client: HandlerInheritance<crate::config::permissions::HttpClientPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_http_framework")]
    pub http_framework: HandlerInheritance<crate::config::permissions::HttpFrameworkPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_runtime")]
    pub runtime: HandlerInheritance<crate::config::permissions::RuntimePermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_supervisor")]
    pub supervisor: HandlerInheritance<crate::config::permissions::SupervisorPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_store")]
    pub store: HandlerInheritance<crate::config::permissions::StorePermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_timing")]
    pub timing: HandlerInheritance<crate::config::permissions::TimingPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_process")]
    pub process: HandlerInheritance<crate::config::permissions::ProcessPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_environment")]
    pub environment: HandlerInheritance<crate::config::permissions::EnvironmentPermissions>,
    #[serde(default, skip_serializing_if = "is_inherit_random")]
    pub random: HandlerInheritance<crate::config::permissions::RandomPermissions>,
}

/// Apply inheritance policy to calculate effective permissions
pub fn apply_inheritance_policy<T: Clone + RestrictWith<T>>(
    parent_capability: &Option<T>,
    policy: &HandlerInheritance<T>,
) -> Option<T> {
    match policy {
        HandlerInheritance::Inherit => parent_capability.clone(),
        HandlerInheritance::Disallow => None,
        HandlerInheritance::Restrict(restriction) => parent_capability
            .as_ref()
            .map(|parent| parent.restrict_with(restriction)),
    }
}
