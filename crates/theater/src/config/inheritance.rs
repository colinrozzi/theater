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

/// Per-handler permission inheritance policies
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandlerPermissionPolicy {
    #[serde(default)]
    pub message_server: HandlerInheritance<crate::config::permissions::MessageServerPermissions>,
    #[serde(default)]
    pub file_system: HandlerInheritance<crate::config::permissions::FileSystemPermissions>,
    #[serde(default)]
    pub http_client: HandlerInheritance<crate::config::permissions::HttpClientPermissions>,
    #[serde(default)]
    pub http_framework: HandlerInheritance<crate::config::permissions::HttpFrameworkPermissions>,
    #[serde(default)]
    pub runtime: HandlerInheritance<crate::config::permissions::RuntimePermissions>,
    #[serde(default)]
    pub supervisor: HandlerInheritance<crate::config::permissions::SupervisorPermissions>,
    #[serde(default)]
    pub store: HandlerInheritance<crate::config::permissions::StorePermissions>,
    #[serde(default)]
    pub timing: HandlerInheritance<crate::config::permissions::TimingPermissions>,
    #[serde(default)]
    pub process: HandlerInheritance<crate::config::permissions::ProcessPermissions>,
    #[serde(default)]
    pub environment: HandlerInheritance<crate::config::permissions::EnvironmentPermissions>,
    #[serde(default)]
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
