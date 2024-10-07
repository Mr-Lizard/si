use serde::{Deserialize, Serialize};
use telemetry::prelude::*;
use telemetry_utils::metric;

use crate::{BeforeFunction, ComponentView, CycloneRequestable};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementRequest {
    pub execution_id: String,
    pub handler: String,
    pub code_base64: String,
    pub this_component: ComponentView,
    pub before: Vec<BeforeFunction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementResultSuccess {
    pub execution_id: String,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl CycloneRequestable for ManagementRequest {
    type Response = ManagementResultSuccess;

    fn execution_id(&self) -> &str {
        &self.execution_id
    }

    fn websocket_path(&self) -> &str {
        "/execute/management"
    }

    fn inc_run_metric(&self) {
        metric!(counter.function_run.management = 1);
    }

    fn dec_run_metric(&self) {
        metric!(counter.function_run.management = -1);
    }
}