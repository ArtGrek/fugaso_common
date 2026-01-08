use std::collections::{BTreeMap, HashMap};

use fugaso_math::{
    config::BaseConfig,
    protocol::{deserialize_lines, deserialize_vec_reels, serialize_vec_reels},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MegaThunderConfig {
    #[serde(
        deserialize_with = "deserialize_vec_reels",
        serialize_with = "serialize_vec_reels"
    )]
    pub reels: Vec<Vec<Vec<char>>>,
    #[serde(deserialize_with = "deserialize_lines")]
    pub lines: Vec<Vec<usize>>,
    pub wins: HashMap<char, HashMap<usize, i32>>,
    pub dist_coin: Vec<BTreeMap<i32, i32>>,
    pub dist_over: BTreeMap<i32, usize>,
    #[serde(default)]
    pub dist_base_category: BTreeMap<i32, usize>,
    #[serde(default)]
    pub stop_factor: i32,
    pub bet_counters: Vec<usize>,
    #[serde(default)]
    pub dist_crown_2x: (usize, usize),
    #[serde(default)]
    pub dist_coin_ultra: BTreeMap<i32, i32>,
    pub map_jack: HashMap<char, i32>,
    pub dist_new_coin: Vec<Vec<f64>>,
}

impl BaseConfig for MegaThunderConfig {
    fn reels(&self) -> &Vec<Vec<Vec<char>>> {
        &self.reels
    }
}

pub mod mega_thunder {
    use std::sync::Arc;

    use lazy_static::lazy_static;

    use super::MegaThunderConfig;
    use fugaso_math::config::ReelDist;

    pub const BASE_CATEGORY: usize = 0;
    pub const BONUS_OFFSET: usize = 1;
    pub const BONUS_COUNT: i32 = 3;
    pub const SYM_NONE: char = 'P';
    pub const SYM_WILD: char = 'I';
    pub const SYM_COLLECT: char = 'J';
    pub const SYM_COINS: [char; 5] = ['K', 'L', 'M', 'N', 'O']; //coin & jackpots
    pub const ROWS: usize = 3;

    lazy_static! {
        pub static ref CFG: Arc<MegaThunderConfig> = {
            let json = include_str!("resources/mega_thunder.json");
            let r = serde_json::from_str(json).expect("error parse config");
            Arc::new(r)
        };
        pub static ref REELS_CFG: Arc<ReelDist> = {
            let json = include_str!("resources/mega_thunder_reels.json");
            let r = serde_json::from_str(json).expect("error parse config");
            Arc::new(r)
        };
    }

    pub fn is_coin(c: char) -> bool {
        c >= SYM_COINS[0] && c <= SYM_COINS[SYM_COINS.len() - 1]
    }
}
