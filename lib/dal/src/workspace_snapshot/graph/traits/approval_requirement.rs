use serde::{Deserialize, Serialize};
use si_events::workspace_snapshot::EntityKind;
use si_id::{ApprovalRequirementDefinitionId, EntityId, UserPk, WorkspacePk};

use crate::workspace_snapshot::graph::{detector::Change, WorkspaceSnapshotGraphResult};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequirementPermissionLookup {
    pub object_type: String,
    pub object_id: String,
    pub permission: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalRequirementApprover {
    User(UserPk),
    PermissionLookup(ApprovalRequirementPermissionLookup),
}

#[derive(Debug, Clone)]
pub struct ApprovalRequirementRule {
    pub entity_id: EntityId,
    pub entity_kind: EntityKind,
    pub minimum: usize,
    pub approvers: Vec<ApprovalRequirementApprover>,
}

#[derive(Debug)]
pub struct ApprovalRequirementsBag {
    pub entity_id: EntityId,
    pub entity_kind: EntityKind,
    pub explicit_approval_requirement_definition_ids: Vec<ApprovalRequirementDefinitionId>,
    pub virtual_approval_requirement_rules: Vec<ApprovalRequirementRule>,
}

pub trait ApprovalRequirementExt {
    fn approval_requirements_for_changes(
        &self,
        workspace_id: WorkspacePk,
        changes: &[Change],
    ) -> WorkspaceSnapshotGraphResult<Vec<ApprovalRequirementsBag>>;
}
