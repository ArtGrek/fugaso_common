use std::collections::{BTreeMap, HashMap};

use fugaso_math::{
    config::BaseConfig,
    protocol::{deserialize_lines, deserialize_vec_reels, serialize_vec_reels},
};
use serde::{Deserialize, Serialize};

use crate::protocol::OverKind;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ThunderExpressConfig {
    #[serde(deserialize_with = "deserialize_vec_reels", serialize_with = "serialize_vec_reels")]
    pub reels: Vec<Vec<Vec<char>>>,
    #[serde(deserialize_with = "deserialize_lines")]
    pub lines: Vec<Vec<usize>>,
    pub wins: HashMap<char, HashMap<usize, i32>>,
    pub dist_coin: Vec<BTreeMap<i32, i32>>,
    pub dist_over: Vec<BTreeMap<i32, usize>>,
    #[serde(default)]
    pub dist_base_category: BTreeMap<i32, usize>,
    #[serde(default)]
    pub stop_factor: i32,
    pub bet_counters: Vec<usize>,
    #[serde(default)]
    pub dist_crown_2x: (usize, usize),
    #[serde(default)]
    pub dist_coin_ultra: BTreeMap<i32, i32>,
    pub map_jack: HashMap<i32, char>,
}

impl BaseConfig for ThunderExpressConfig {
    fn reels(&self) -> &Vec<Vec<Vec<char>>> {
        &self.reels
    }
}

pub mod thunder_express {
    use std::sync::Arc;

    use lazy_static::lazy_static;

    use super::ThunderExpressConfig;
    use fugaso_math::config::ReelDist;

    pub const BASE_CATEGORY: usize = 0;
    pub const BONUS_OFFSET: usize = 2;
    pub const NUM_CATEGORIES: usize = 2;
    pub const BONUS_COUNT: i32 = 3;
    pub const SYM_NONE: char = 'P';
    pub const SYM_WILD: char = 'I';
    pub const SYM_COLLECT: char = 'J';
    pub const SYM_COINS: [char; 5] = ['K', 'L', 'M', 'N', 'O']; //coin & jackpots
    pub const ROWS: usize = 3;

    lazy_static! {
        pub static ref CFG: Arc<ThunderExpressConfig> = {
            let json = include_str!("resources/thunder_express.json");
            let r = serde_json::from_str(json).expect("error parse config");
            Arc::new(r)
        };
        pub static ref REELS_CFG: Arc<ReelDist> = {
            let json = include_str!("resources/thunder_express_reels.json");
            let r = serde_json::from_str(json).expect("error parse config");
            Arc::new(r)
        };
    }

    pub fn is_coin(c: char) -> bool {
        c >= SYM_COINS[0] && c <= SYM_COINS[SYM_COINS.len() - 1]
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BonanzaLinkCashConfig {
    #[serde(deserialize_with = "deserialize_vec_reels", serialize_with = "serialize_vec_reels")]
    pub reels: Vec<Vec<Vec<char>>>,
    #[serde(deserialize_with = "deserialize_lines")]
    pub lines: Vec<Vec<usize>>,
    pub wins: HashMap<char, HashMap<usize, i32>>,
    pub dist_coin: BTreeMap<i32, i32>,
    pub bet_counters: Vec<usize>,
    pub dist_shift: (i32, i32),
    pub dist_pull: (i32, i32),
    pub dist_over: BTreeMap<i32, OverKind>,
    pub dist_bang: BTreeMap<i32, usize>,
    pub stop_factor: i32,
    pub dist_wilds: Vec<(i32, i32)>,
}

impl BaseConfig for BonanzaLinkCashConfig {
    fn reels(&self) -> &Vec<Vec<Vec<char>>> {
        &self.reels
    }
}

pub mod bonanza_1000 {
    use std::sync::Arc;

    use lazy_static::lazy_static;

    use super::BonanzaLinkCashConfig;

    pub const BASE_CATEGORY: usize = 0;
    pub const X5_CATEGORY: usize = 1;
    pub const FREE_CATEGORY: usize = 2;
    pub const FREE_GAME_FACTOR: i32 = 5;
    pub const FREE_GAMES_LEVEL: i32 = 10;
    pub const SYM_SCAT: char = 'A';
    pub const SYM_WILD: char = 'B';
    pub const SYM_COIN: char = 'G';
    pub const LEVEL_MAX: usize = 4;
    pub const STEPS_ON_LEVEL: usize = 4;
    pub const MULT_LEVELS: [i32; 4] = [1, 2, 3, 10];
    pub const BET_LEVELS: [i32; 2] = [10, 15];
    pub const SCATTERS_FOR_FREE: usize = 3;

    lazy_static! {
        pub static ref CFG: Arc<BonanzaLinkCashConfig> = {
            let json = include_str!("resources/bonanza_link_1000.json");
            let r = serde_json::from_str(json).expect("error parse config");
            Arc::new(r)
        };
    }
}
