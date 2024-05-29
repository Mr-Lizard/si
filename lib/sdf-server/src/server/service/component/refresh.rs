use axum::extract::OriginalUri;
use axum::Json;

use dal::{
    action::prototype::ActionKind, action::prototype::ActionPrototype, action::Action, Component,
    ComponentId, Visibility,
};
use serde::{Deserialize, Serialize};

use super::ComponentResult;
use crate::server::extract::{AccessBuilder, HandlerContext, PosthogClient};
use crate::server::tracking::track;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub component_id: ComponentId,
    #[serde(flatten)]
    pub visibility: Visibility,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResponse {
    pub success: bool,
}

pub async fn refresh(
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    PosthogClient(posthog_client): PosthogClient,
    OriginalUri(original_uri): OriginalUri,
    Json(request): Json<RefreshRequest>,
) -> ComponentResult<Json<RefreshResponse>> {
    let ctx = builder.build(request_ctx.build(request.visibility)).await?;

    let component_ids = vec![request.component_id];

    track(
        &posthog_client,
        &ctx,
        &original_uri,
        "refresh_resource",
        serde_json::json!({
            "component_ids": &component_ids,
        }),
    );

    // Parallelizes resource refreshing
    for component_id in component_ids {
        let variant = Component::schema_variant_for_component_id(&ctx, component_id).await?;

        let all_prototypes_for_variant: Vec<ActionPrototype> =
            ActionPrototype::for_variant(&ctx, variant.id()).await?;
        for prototype in all_prototypes_for_variant {
            if prototype.kind == ActionKind::Refresh {
                Action::new(&ctx, prototype.id(), Some(component_id)).await?;
            }
        }
    }

    ctx.commit().await?;

    Ok(Json(RefreshResponse { success: true }))
}
