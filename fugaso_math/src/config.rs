use crate::protocol::{deserialize_lines, deserialize_vec_reels, serialize_vec_reels};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub type ReelDist = Vec<Vec<BTreeMap<i32, Vec<char>>>>;

pub trait BaseConfig {
    fn reels(&self) -> &Vec<Vec<Vec<char>>>;
}

pub trait LinkConfig: BaseConfig {
    fn dist_over(&self) -> &BTreeMap<i32, usize>;

    fn dist_coin(&self) -> &BTreeMap<i32, i32>;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MathConfig {
    #[serde(
        deserialize_with = "deserialize_vec_reels",
        serialize_with = "serialize_vec_reels"
    )]
    pub reels: Vec<Vec<Vec<char>>>,
    #[serde(deserialize_with = "deserialize_lines")]
    pub lines: Vec<Vec<usize>>,
    pub wins: HashMap<char, HashMap<usize, i32>>,
}

impl BaseConfig for MathConfig {
    fn reels(&self) -> &Vec<Vec<Vec<char>>> {
        &self.reels
    }
}