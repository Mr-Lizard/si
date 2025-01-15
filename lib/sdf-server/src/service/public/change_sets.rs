use axum::{
    extract::Host,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use dal::{change_set::ChangeSet, WsEvent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use si_events::audit_log::AuditLogKind;
use thiserror::Error;

use crate::AppState;
use crate::{
    extract::{
        change_set::{ChangeSetDalContext, TargetChangeSetIdFromPath},
        workspace::{WorkspaceAuthorization, WorkspaceDalContext},
        PosthogEventTracker,
    },
    service::v2::change_set::post_to_webhook,
};

#[remain::sorted]
#[derive(Debug, Error)]
pub enum ChangeSetsError {
    #[error("dal change set error: {0}")]
    ChangeSet(#[from] dal::ChangeSetError),
    #[error("change set apply error: {0}")]
    ChangeSetApply(#[from] dal::ChangeSetApplyError),
    #[error("change set service error: {0}")]
    ChangeSetService(#[from] crate::service::v2::change_set::Error),
    #[error("func error: {0}")]
    Func(#[from] dal::FuncError),
    #[error("schema error: {0}")]
    Schema(#[from] dal::SchemaError),
    #[error("schema variant error: {0}")]
    SchemaVariant(#[from] dal::SchemaVariantError),
    #[error("transactions error: {0}")]
    Transactions(#[from] dal::TransactionsError),
    #[error("workspace snapshot error: {0}")]
    WorkspaceSnapshot(#[from] dal::WorkspaceSnapshotError),
    #[error("ws event error: {0}")]
    WsEvent(#[from] dal::WsEventError),
}

type Result<T> = std::result::Result<T, ChangeSetsError>;

impl IntoResponse for ChangeSetsError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

// /api/public/workspaces/:workspace_id/change-sets
pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(create_change_set)).nest(
        "/:change_set_id",
        Router::new()
            .nest("/components", super::components::routes())
            .nest("/management", super::management::routes())
            .route("/request_approval", post(request_approval))
            .route_layer(middleware::from_extractor::<TargetChangeSetIdFromPath>()),
    )
}

async fn create_change_set(
    WorkspaceDalContext(ref ctx): WorkspaceDalContext,
    tracker: PosthogEventTracker,
    Json(payload): Json<CreateChangeSetRequest>,
) -> Result<Json<CreateChangeSetResponse>> {
    let change_set = ChangeSet::fork_head(ctx, &payload.change_set_name).await?;

    tracker.track(ctx, "create_change_set", json!(payload));

    ctx.write_audit_log(AuditLogKind::CreateChangeSet, payload.change_set_name)
        .await?;

    WsEvent::change_set_created(ctx, change_set.id)
        .await?
        .publish_on_commit(ctx)
        .await?;

    ctx.commit_no_rebase().await?;

    Ok(Json(CreateChangeSetResponse { change_set }))
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateChangeSetRequest {
    change_set_name: String,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateChangeSetResponse {
    change_set: ChangeSet,
}

async fn request_approval(
    ChangeSetDalContext(ref mut ctx): ChangeSetDalContext,
    WorkspaceAuthorization { user, .. }: WorkspaceAuthorization,
    tracker: PosthogEventTracker,
    Host(host_name): Host,
) -> Result<()> {
    let workspace_pk = ctx.workspace_pk()?;
    let mut change_set = ctx.change_set()?.clone();
    let change_set_id = change_set.id;
    let old_status = change_set.status;

    change_set.request_change_set_approval(ctx).await?;

    tracker.track(
        ctx,
        "request_change_set_approval",
        serde_json::json!({
            "change_set": change_set.id,
        }),
    );
    // TODO change to get_by_id when https://github.com/systeminit/si/pull/5261 lands
    let change_set_view = ChangeSet::get_by_id(ctx, change_set_id)
        .await?
        .into_frontend_type(ctx)
        .await?;

    let change_set_url = format!(
        "https://{}/w/{}/{}",
        host_name,
        ctx.workspace_pk()?,
        change_set_id
    );
    let message = format!(
        "{} requested an approval of change set {}: {}",
        user.email(),
        change_set_view.name.clone(),
        change_set_url
    );
    post_to_webhook(ctx, workspace_pk, message.as_str()).await?;

    ctx.write_audit_log(
        AuditLogKind::RequestChangeSetApproval {
            from_status: old_status.into(),
        },
        change_set_view.name.clone(),
    )
    .await?;

    WsEvent::change_set_status_changed(ctx, old_status, change_set_view)
        .await?
        .publish_on_commit(ctx)
        .await?;

    ctx.commit().await?;

    Ok(())
}
