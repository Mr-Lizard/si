use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{header::HeaderMap, request::Parts},
    RequestPartsExt as _,
};
use dal::{User, UserPk, WorkspacePk};
use derive_more::{Deref, Into};
use si_jwt_public_key::SiJwtClaimRole;
use std::str::FromStr;
use ulid::Ulid;

use crate::app_state::AppState;

use super::{
    bad_request, internal_error,
    request::{RequestUlidFromHeader, ValidatedToken},
    services::HandlerContext,
    unauthorized_error, ErrorResponse,
};

///
/// Handles the whole endpoint authorization (checking if the user has access to the target
/// workspace with the desired role, *and* that the user is a member of the workspace).
///
/// Unless you have already used the `TokenParamAccessToken` extractor to get the token from
/// query parameters, this will retrieve the token from the Authorization header.
///
/// Unless you have already used the `AuthorizeForAutomationRole` extractor to check that the
/// token has the automation role, this will check for maximal permissions (the web role).
///
#[derive(Clone, Debug)]
pub struct WorkspaceAuthorization {
    pub user: User,
    pub workspace_id: WorkspacePk,
    pub authorized_role: SiJwtClaimRole,
    pub request_ulid: Option<Ulid>,
}

impl WorkspaceAuthorization {
    pub fn access_builder(&self) -> dal::AccessBuilder {
        self.clone().into()
    }
}

impl From<WorkspaceAuthorization> for dal::AccessBuilder {
    fn from(auth: WorkspaceAuthorization) -> dal::AccessBuilder {
        dal::AccessBuilder::new(
            dal::Tenancy::new(auth.workspace_id),
            dal::HistoryActor::from(auth.user.pk()),
            auth.request_ulid,
        )
    }
}

#[async_trait]
impl FromRequestParts<AppState> for WorkspaceAuthorization {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(result) = parts.extensions.get::<Self>() {
            return Ok(result.clone());
        }

        let AuthorizedForRole {
            user_id,
            workspace_id,
            authorized_role,
        } = parts.extract_with_state(state).await?;

        // Get a context associated with the workspace but not the user
        let HandlerContext(builder) = parts.extract_with_state(state).await?;
        let RequestUlidFromHeader(request_ulid) = parts.extract_with_state(state).await?;
        let access_builder =
            dal::AccessBuilder::new(workspace_id.into(), user_id.into(), request_ulid);
        let ctx = builder
            .build_head(access_builder)
            .await
            .map_err(internal_error)?;

        // Check if the user is a member of the workspace (and get the record if so)
        let workspace_members = User::list_members_for_workspace(&ctx, workspace_id.to_string())
            .await
            .map_err(internal_error)?;
        let user = workspace_members
            .into_iter()
            .find(|m| m.pk() == user_id)
            .ok_or_else(|| unauthorized_error("User not a member of the workspace"))?;

        Ok(Self {
            user,
            workspace_id,
            request_ulid,
            authorized_role,
        })
    }
}

///
/// Confirms that the user has been authorized for the desired role for the target workspace.
///
/// Does *not* confirm that the user is a member of the workspace (use WorkspaceMember for that).
///
/// Stores the role that was authorized.
///
/// To authorize for something other than web role, use the `AuthorizeForAutomationRole` extractor.
///
#[derive(Clone, Copy, Debug)]
struct AuthorizedForRole {
    user_id: UserPk,
    workspace_id: WorkspacePk,
    authorized_role: SiJwtClaimRole,
}

impl AuthorizedForRole {
    async fn authorize_for(
        parts: &mut Parts,
        state: &AppState,
        role: SiJwtClaimRole,
    ) -> Result<AuthorizedForRole, ErrorResponse> {
        // This must not be done twice.
        if parts.extensions.get::<AuthorizedForRole>().is_some() {
            return Err(internal_error(
                "Must only specify explicit endpoint authorization once",
            ));
        }

        let token = ValidatedToken::from_request_parts(parts, state).await?.0;

        // Validate the workspace_id is the same as the target workspace
        let workspace_id = TargetWorkspaceId::from_request_parts(parts, state).await?.0;
        if workspace_id != token.custom.workspace_id() {
            return Err(unauthorized_error("Not authorized for workspace"));
        }

        // Validate the role
        if !token.custom.authorized_for(role) {
            return Err(unauthorized_error("Not authorized for role"));
        }

        // Stash the authorization
        let result = AuthorizedForRole {
            user_id: token.custom.user_id(),
            workspace_id,
            authorized_role: role,
        };
        parts.extensions.insert(result);

        Ok(result)
    }
}

#[async_trait]
impl FromRequestParts<AppState> for AuthorizedForRole {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(&result) = parts.extensions.get::<AuthorizedForRole>() {
            return Ok(result);
        }
        AuthorizedForRole::authorize_for(parts, state, SiJwtClaimRole::Web).await
    }
}

///
/// Ensure the user has been authorized for the web role for the target workspace.
///
/// Does *not* validate that the user is a member of the workspace. WorkspaceAuthorization
/// handles that.
///
#[derive(Clone, Copy, Debug)]
pub struct AuthorizedForWebRole;

#[async_trait]
impl FromRequestParts<AppState> for AuthorizedForWebRole {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        AuthorizedForRole::authorize_for(parts, state, SiJwtClaimRole::Web).await?;
        Ok(Self)
    }
}

///
/// A user who has been authorized for the given workspace for the web role.
///
#[derive(Clone, Copy, Debug)]
pub struct AuthorizedForAutomationRole;

#[async_trait]
impl FromRequestParts<AppState> for AuthorizedForAutomationRole {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        AuthorizedForRole::authorize_for(parts, state, SiJwtClaimRole::Automation).await?;
        Ok(Self)
    }
}

/// The target workspace id from the path or header.
///
/// *Not* validated against the token's workspace_id. AuthorizedForRole does that.
///
/// Defaults to getting the workspace id from the path (/workspaces/:workspace_id). Use the
/// `TargetWorkspaceIdFromHeader` extractor to get the workspace id from the header.
///
#[derive(Clone, Debug, Deref, Copy, Into)]
pub struct TargetWorkspaceId(pub WorkspacePk);

impl TargetWorkspaceId {
    fn set(parts: &mut Parts, workspace_id: WorkspacePk) -> Result<WorkspacePk, ErrorResponse> {
        // This must not be done twice.
        if parts.extensions.get::<TargetWorkspaceId>().is_some() {
            return Err(internal_error("Must only specify workspace ID once"));
        }

        parts.extensions.insert(TargetWorkspaceId(workspace_id));
        Ok(workspace_id)
    }
}

#[async_trait]
impl FromRequestParts<AppState> for TargetWorkspaceId {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(*parts
            .extensions
            .get::<TargetWorkspaceId>()
            .ok_or_else(|| internal_error("No workspace ID. Endpoints must call an extractor like TargetWorkspaceIdFromPath TargetWorkspaceFromToken to get the workspace ID."))?)
    }
}

/// Extracts a workspace id from a header, fail if not found
#[derive(Clone, Debug, Deref, Copy, Into)]
pub struct TargetWorkspaceIdFromHeader(WorkspacePk);

impl TargetWorkspaceIdFromHeader {
    pub fn extract(headers: &HeaderMap) -> Result<Option<WorkspacePk>, ErrorResponse> {
        match headers.get("X-Workspace-Id") {
            None => Ok(None),
            Some(workspace_id_header) => {
                let workspace_id_string = workspace_id_header.to_str().map_err(bad_request)?;
                let workspace_id =
                    WorkspacePk::from_str(workspace_id_string).map_err(bad_request)?;
                Ok(Some(workspace_id))
            }
        }
    }
}

#[async_trait]
impl FromRequestParts<AppState> for TargetWorkspaceIdFromHeader {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let workspace_id = TargetWorkspaceIdFromHeader::extract(&parts.headers)?
            .ok_or_else(|| unauthorized_error("no Authorization header"))?;

        Ok(Self(TargetWorkspaceId::set(parts, workspace_id)?))
    }
}

/// Extracts a workspace id from the token. TEMPORARY until web and dal have both redeployed
#[derive(Clone, Debug, Deref, Copy, Into)]
pub struct TargetWorkspaceIdFromToken(WorkspacePk);

#[async_trait]
impl FromRequestParts<AppState> for TargetWorkspaceIdFromToken {
    type Rejection = ErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = ValidatedToken::from_request_parts(parts, state).await?.0;
        Ok(Self(TargetWorkspaceId::set(
            parts,
            token.custom.workspace_id(),
        )?))
    }
}
