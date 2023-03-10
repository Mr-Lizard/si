use serde::{Deserialize, Serialize};
use si_data_nats::NatsError;
use si_data_pg::PgError;
use telemetry::prelude::*;
use thiserror::Error;

use crate::{
    pk, standard_model, standard_model_accessor_ro, DalContext, HistoryActor, HistoryEvent,
    HistoryEventError, KeyPair, KeyPairError, StandardModelError, Tenancy, Timestamp,
    TransactionsError, User, UserError,
};

const WORKSPACE_GET_BY_PK: &str = include_str!("queries/workspace/get_by_pk.sql");
const WORKSPACE_FIND_BY_NAME: &str = include_str!("queries/workspace/find_by_name.sql");

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error(transparent)]
    KeyPair(#[from] KeyPairError),
    #[error(transparent)]
    Transactions(#[from] TransactionsError),
    #[error(transparent)]
    User(#[from] UserError),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Pg(#[from] PgError),
    #[error(transparent)]
    Nats(#[from] NatsError),
    #[error(transparent)]
    HistoryEvent(#[from] HistoryEventError),
    #[error(transparent)]
    StandardModel(#[from] StandardModelError),
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

pk!(WorkspacePk);

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSignup {
    pub key_pair: KeyPair,
    pub user: User,
    pub workspace: Workspace,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pk: WorkspacePk,
    name: String,
    #[serde(flatten)]
    timestamp: Timestamp,
}

impl Workspace {
    pub fn pk(&self) -> &WorkspacePk {
        &self.pk
    }

    #[instrument(skip_all)]
    pub async fn builtin(ctx: &DalContext) -> WorkspaceResult<Self> {
        let row = ctx
            .txns()
            .pg()
            .query_one(
                "SELECT object FROM workspace_find_or_create_builtin_v1()",
                &[],
            )
            .await?;

        let object = standard_model::object_from_row(row)?;
        Ok(object)
    }

    #[instrument(skip_all)]
    pub async fn new(ctx: &mut DalContext, name: impl AsRef<str>) -> WorkspaceResult<Self> {
        let name = name.as_ref();
        let row = ctx
            .txns()
            .pg()
            .query_one("SELECT object FROM workspace_create_v1($1)", &[&name])
            .await?;

        // Inlined `finish_create_from_row`

        let json: serde_json::Value = row.try_get("object")?;
        let object: Self = serde_json::from_value(json)?;

        ctx.update_tenancy(Tenancy::new(object.pk));

        let _history_event = HistoryEvent::new(
            ctx,
            "workspace.create".to_owned(),
            "Workspace created".to_owned(),
            &serde_json::json![{ "visibility": ctx.visibility() }],
        )
        .await?;
        Ok(object)
    }

    pub async fn signup(
        ctx: &mut DalContext,
        workspace_name: impl AsRef<str>,
        user_name: impl AsRef<str>,
        user_email: impl AsRef<str>,
        user_password: impl AsRef<str>,
    ) -> WorkspaceResult<WorkspaceSignup> {
        let workspace = Workspace::new(ctx, workspace_name).await?;
        let key_pair = KeyPair::new(ctx, "default").await?;

        let user = User::new(ctx, &user_name, &user_email, &user_password).await?;
        ctx.update_history_actor(HistoryActor::User(user.pk()));

        ctx.import_builtins().await?;

        Ok(WorkspaceSignup {
            key_pair,
            user,
            workspace,
        })
    }

    pub async fn find_by_name(ctx: &DalContext, name: &str) -> WorkspaceResult<Option<Workspace>> {
        let row = ctx
            .txns()
            .pg()
            .query_opt(WORKSPACE_FIND_BY_NAME, &[&name])
            .await?;
        let result = standard_model::option_object_from_row(row)?;
        Ok(result)
    }

    pub async fn get_by_pk(ctx: &DalContext, pk: &WorkspacePk) -> WorkspaceResult<Workspace> {
        let row = ctx
            .txns()
            .pg()
            .query_one(WORKSPACE_GET_BY_PK, &[&pk])
            .await?;
        let result = standard_model::object_from_row(row)?;
        Ok(result)
    }

    standard_model_accessor_ro!(name, String);
}
