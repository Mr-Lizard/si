use axum::Json;
use dal::{User, Workspace};
use serde::{Deserialize, Serialize};

use super::SessionResult;
use crate::server::extract::{AccessBuilder, Authorization, HandlerContext};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreAuthenticationResponse {
    pub user: User,
    pub workspace: Workspace,
}

pub async fn restore_authentication(
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Authorization(claim): Authorization,
) -> SessionResult<Json<RestoreAuthenticationResponse>> {
    let ctx = builder.build(request_ctx.build_head()).await?;

    let workspace = Workspace::get_by_pk(&ctx, &claim.workspace_pk).await?;

    let user = User::get_by_pk(&ctx, claim.user_pk).await?;

    let reply = RestoreAuthenticationResponse { user, workspace };

    Ok(Json(reply))
}
