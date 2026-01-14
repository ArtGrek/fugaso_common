use essential_core::{err_on, error::ServerError};
use fugaso_math::protocol::DatabaseStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ThunderExpressInfo {
    #[serde(default)]
    pub total: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mults: Vec<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mults1: Vec<Vec<i32>>,
    pub respins: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
    #[serde(default)]
    pub accum: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i32>,
}

impl DatabaseStore for ThunderExpressInfo {
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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BonanzaLink1000Info {
    #[serde(default)]
    pub total: i64,
    pub respins: i32,
    #[serde(default)]
    pub accum: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mults: Vec<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<OverBonus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<Stage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_max: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Stage {
    pub total: usize,
    pub level: usize,
    pub current: usize,
    pub shift: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OverBonus {
    pub kind: OverKind,
    pub grid: Vec<Vec<char>>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub enum OverKind {
    #[default]
    Shoot,
    Pull,
    Bang,
}

impl DatabaseStore for BonanzaLink1000Info {
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
