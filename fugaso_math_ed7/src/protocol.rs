use essential_core::{err_on, error::ServerError};
use fugaso_math::protocol::DatabaseStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiftItem {
    pub p: (usize, usize),
    pub m: i32,
    pub v: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MegaThunderInfo {
    #[serde(default)]
    pub total: i64,
    pub respins: i32,
    #[serde(default)]
    pub accum: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
    #[serde(default = "default_array", skip_serializing_if = "Vec::is_empty")]
    pub mults: Vec<Vec<i32>>,
    #[serde(default = "default_array", skip_serializing_if = "Vec::is_empty")]
    pub lifts: Vec<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifts_new: Vec<LiftItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grand: Vec<i32>,
}

fn default_array() -> Vec<Vec<i32>> {
    vec![vec![0; 3]; 5]
}

impl DatabaseStore for MegaThunderInfo {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        serde_json::from_str(&value).map_err(|e| err_on!(e))
    }

    fn to_db(&self) -> Result<String, ServerError> {
        serde_json::to_string(self).map_err(|e| err_on!(e))
    }

    fn respins(&self) -> i32 {
        self.respins
    }

    fn stop(&self) -> i32 {
        self.stop.unwrap_or(0)
    }
}
