mod prop_array;
mod prop_boolean;
mod prop_map;
mod prop_number;
mod prop_object;
mod prop_string;

use crate::SchemaResult;
pub use prop_array::PropArray;
pub use prop_boolean::PropBoolean;
pub use prop_map::PropMap;
pub use prop_number::PropNumber;
pub use prop_object::PropObject;
pub use prop_string::PropString;
use serde::{Deserialize, Serialize};
use si_data::PgTxn;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

const PROP_BY_ID: &str = include_str!("../queries/prop_by_id.sql");

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Prop {
    String(PropString),
    Number(PropNumber),
    Boolean(PropBoolean),
    Object(PropObject),
    Array(PropArray),
    Map(PropMap),
}

impl Prop {
    pub fn id(&self) -> &str {
        match self {
            Prop::String(p) => p.id.as_str(),
            Prop::Number(p) => p.id.as_str(),
            Prop::Boolean(p) => p.id.as_str(),
            Prop::Object(p) => p.id.as_str(),
            Prop::Array(p) => p.id.as_str(),
            Prop::Map(p) => p.id.as_str(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Prop::String(p) => p.name.as_ref(),
            Prop::Number(p) => p.name.as_str(),
            Prop::Boolean(p) => p.name.as_str(),
            Prop::Object(p) => p.name.as_str(),
            Prop::Array(p) => p.name.as_str(),
            Prop::Map(p) => p.name.as_str(),
        }
    }

    pub fn parent_id(&self) -> Option<&str> {
        match self {
            Prop::String(p) => p.parent_id.as_deref(),
            Prop::Number(p) => p.parent_id.as_deref(),
            Prop::Boolean(p) => p.parent_id.as_deref(),
            Prop::Object(p) => p.parent_id.as_deref(),
            Prop::Array(p) => p.parent_id.as_deref(),
            Prop::Map(p) => p.parent_id.as_deref(),
        }
    }

    pub async fn get_by_id(txn: &PgTxn<'_>, id: impl AsRef<str>) -> SchemaResult<Self> {
        let id = id.as_ref();
        let row = txn.query_one(PROP_BY_ID, &[&id]).await?;
        let json: serde_json::Value = row.try_get("object")?;
        let object = serde_json::from_value(json)?;
        Ok(object)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SchemaMap(HashMap<String, Prop>);

impl Deref for SchemaMap {
    type Target = HashMap<String, Prop>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SchemaMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl SchemaMap {
    pub fn new() -> SchemaMap {
        SchemaMap(HashMap::new())
    }

    pub fn find_prop_by_name(&self, parent_id: Option<&str>, name: impl AsRef<str>) -> Option<&Prop> {
        let name = name.as_ref();
        self.values().find(|p| p.parent_id() == parent_id && p.name() == name)
    }

    pub fn find_item_prop_for_parent(&self, parent_id: impl AsRef<str>) -> Option<&Prop> {
        let parent_id = parent_id.as_ref();
        self.values().find(|p| p.parent_id() == Some(parent_id))
    }
}