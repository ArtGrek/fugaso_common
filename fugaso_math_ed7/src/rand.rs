use std::sync::Arc;

use essential_rand::{err_on, error::RandError, random::RandomGenerator};
use fugaso_math::{
    config::ReelDist,
    rand::{BaseRandom, GroupRandom, Result},
};
use log::debug;
use mockall::*;

use crate::config::{mega_thunder, MegaThunderConfig};
use crate::protocol::LiftItem;

#[automock]
pub trait MegaThunderRand {
    fn rand_buy_spin_grid(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)>; //?

    fn rand_spin_grid(&mut self, category: usize, combos: Option<Vec<usize>>,) -> Result<(Vec<usize>, Vec<Vec<char>>)>;
    fn rand_grid_coins(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>>;
    fn rand_grid_jackpots(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>>;
    fn rand_grid_lifts(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>>;
    fn rand_spin_over(&mut self, grid: &Vec<Vec<char>>) -> Result<Option<Vec<Vec<char>>>>;

    fn rand_respin_grid(&mut self,category: usize,combos: Option<Vec<usize>>,) -> (Vec<usize>, Vec<Vec<char>>);

    fn rand_coins_values(&mut self, grid: &Vec<Vec<char>>, mults: &Vec<Vec<i32>>, counter_idx: usize) -> Option<Vec<Vec<i32>>>;
    fn rand_jackpots_values(&mut self, grid: &Vec<Vec<char>>, mults: &Vec<Vec<i32>>, counter_idx: usize) -> Option<Vec<Vec<i32>>>;
    fn rand_lifts_values_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<LiftItem>>;

}

pub struct MegaThunderRandom {
    pub p: GroupRandom<MegaThunderConfig>,
}

impl MegaThunderRandom {
    pub fn new(config: Arc<MegaThunderConfig>, reels_cfg: Arc<ReelDist>) -> Self {
        Self {
            p: GroupRandom {
                reels_cfg,
                base: BaseRandom {
                    rand: RandomGenerator::new(),
                    rows: mega_thunder::ROWS,
                    config,
                },
            },
        }
    }
}

impl MegaThunderRand for MegaThunderRandom {

    fn rand_buy_spin_grid(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let reels = &self.p.reels_cfg[category];
        let stops_grid = reels.iter().enumerate().map(|(_c, dist)| {
            let mut m = dist.iter().enumerate().filter_map(|(i, (_k, v))| {
                if v.iter().any(|s| mega_thunder::is_specials(*s)) {Some(i)} else {None}
            }).collect::<Vec<_>>();
            let idx = self.p.base.rand.rand_vec_remove(&mut m)?;
            dist.iter().enumerate().find_map(|(i, (_k, v))| {
                if i == idx {Some((idx, v.clone()))} else {None}
            }).ok_or_else(|| err_on!("random find item error!"))
        }).collect::<Result<Vec<_>>>()?;
        let (stops, grid) = (
            stops_grid.iter().map(|p| p.0).collect::<Vec<_>>(),
            stops_grid.into_iter().map(|p| p.1).collect::<Vec<_>>(),
        );
        Ok((stops, grid))
    }

    fn rand_spin_grid(&mut self, category: usize, combos: Option<Vec<usize>>,) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let (stops, grid) = self.p.rand_cols_group(category, combos)?;
        Ok((stops, grid))
    }

    fn rand_grid_coins(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>> {
        let dist_coin = &self.p.base.config.dist_coin;
        let mut result_grid = grid.clone();
        result_grid.iter_mut().for_each(|col| {
            col.iter_mut().for_each(|row| {
                if !mega_thunder::is_specials(*row) {
                    if self.p.base.rand.random(0, dist_coin.1) < dist_coin.0 {*row = mega_thunder::SYM_COIN;}
                }
            });
        });
        if *grid != result_grid {Some(result_grid)} else {None}
    }

    fn rand_grid_jackpots(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>> {
        let dist_jackpot = &self.p.base.config.dist_jackpot;
        let mut result_grid = grid.clone();
        result_grid.iter_mut().for_each(|col| {
            col.iter_mut().for_each(|row| {
                if !mega_thunder::is_specials(*row) {
                    if self.p.base.rand.random(0, dist_jackpot.1) < dist_jackpot.0 {*row = mega_thunder::SYM_JACKPOT;}
                }
            });
        });
        if *grid != result_grid {Some(result_grid)} else {None}
    }

    fn rand_grid_lifts(&mut self, grid: &Vec<Vec<char>>) -> Option<Vec<Vec<char>>> {
        let dist_lift = &self.p.base.config.dist_lift;
        let mut result_grid = grid.clone();
        let mut placed = false;
        for col in result_grid.iter_mut() {
            if placed { break; }
            for row in col.iter_mut() {
                if placed { break; }
                if !mega_thunder::is_specials(*row) {
                    if self.p.base.rand.random(0, dist_lift.1) < dist_lift.0 {
                        *row = mega_thunder::SYM_MULTI;
                        placed = true;
                    }
                }
            }
        }

        if *grid != result_grid { Some(result_grid) } else { None }
    }

    fn rand_spin_over(&mut self, grid: &Vec<Vec<char>>) -> Result<Option<Vec<Vec<char>>>> {
        let over_coins_count = self.p.base.rand.rand_value(&self.p.base.config.dist_over)?;
        let dist_over_symbol = &self.p.base.config.dist_over_symbol;
        if over_coins_count > 0 {
            let mut overlay = grid.clone();
            let mut empty_pos = overlay.iter().enumerate().flat_map(|(col_num, col)| {
                col.iter().enumerate().filter_map(move |(row_num, row)| {
                    if !mega_thunder::is_specials(*row) {Some((col_num, row_num))} else {None}
                })
            }).collect::<Vec<_>>();
            let filled_count = overlay.iter().flat_map(|col| {
                col.iter().filter(|row| {
                    mega_thunder::is_specials(**row)
                })
            }).count();
            debug!("filled: {filled_count} - {overlay:?}");
            let mut over_coins_add_count = over_coins_count - filled_count;
            if over_coins_add_count > 0 {
                while over_coins_add_count > 0 {
                    let (col, row) = self.p.base.rand.rand_vec_remove(&mut empty_pos)?;
                    overlay[col][row] = self.p.base.rand.rand_value(&dist_over_symbol)?;
                    over_coins_add_count -= 1;
                }
                Ok(Some(overlay))
            } else {Ok(None)}
        } else {Ok(None)}
    }

    fn rand_respin_grid(&mut self, category: usize, combos: Option<Vec<usize>>,) -> (Vec<usize>, Vec<Vec<char>>) {
        self.p.rand_cols(category, combos)
    }

    fn rand_coins_values(&mut self, grid: &Vec<Vec<char>>, mults: &Vec<Vec<i32>>, counter_idx: usize) -> Option<Vec<Vec<i32>>> {
        let dist_coin_value = &self.p.base.config.dist_coin_value[counter_idx];
        let mut result_mults = mults.clone();
        grid.iter().enumerate().for_each(|(col_num, col)| {
            col.iter().enumerate().for_each(|(row_num, row)| {
                if *row == mega_thunder::SYM_COIN {if let Ok(coin_value) = self.p.base.rand.rand_value(&dist_coin_value) {result_mults[col_num][row_num] = coin_value}}
            })
        });
        if *mults != result_mults {Some(result_mults)} else {None}
    }

    fn rand_jackpots_values(&mut self, grid: &Vec<Vec<char>>, mults: &Vec<Vec<i32>>, counter_idx: usize) -> Option<Vec<Vec<i32>>> {
        let dist_jackpot_value = &self.p.base.config.dist_jackpot_value[counter_idx];
        let mut result_mults = mults.clone();
        grid.iter().enumerate().for_each(|(col_num, col)| {
            col.iter().enumerate().for_each(|(row_num, row)| {
                if *row == mega_thunder::SYM_JACKPOT {if let Ok(jackpot_value) = self.p.base.rand.rand_value(&dist_jackpot_value) {result_mults[col_num][row_num] = jackpot_value}}
            })
        });
        if *mults != result_mults {Some(result_mults)} else {None}
    }

    fn rand_lifts_values_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<LiftItem>> {
        let dist_lift_symbol = &self.p.base.config.dist_lift_symbol[counter_idx];
        let dist_coin_value = &self.p.base.config.dist_coin_value[counter_idx];
        let dist_jackpot_value = &self.p.base.config.dist_jackpot_value[counter_idx];
        let dist_lift_mult = &self.p.base.config.dist_lift_mult[counter_idx];
        grid.iter().enumerate().flat_map(|(col_num, col)| {
            col.iter().enumerate().filter_map(move |(row_num, row)| {
                if *row == mega_thunder::SYM_MULTI {Some((col_num, row_num))} else {None}
            })
        }).map(|(col, row)| {
            let lift_mult =if let Ok(lift_mult) = self.p.base.rand.rand_value(&dist_lift_mult) {lift_mult} else {0};
            let lift_value = match self.p.base.rand.rand_value(&dist_lift_symbol) {
                Ok(mega_thunder::SYM_COIN) => {if let Ok(coin_value) = self.p.base.rand.rand_value(&dist_coin_value) {coin_value} else {0}},
                Ok(mega_thunder::SYM_JACKPOT) => {if let Ok(jackpot_value) = self.p.base.rand.rand_value(&dist_jackpot_value) {jackpot_value} else {0}},
                _ => {0}
            };
            Ok(LiftItem {
                p: (col, row),
                m: lift_mult,
                v: lift_value,
            })
        }).collect::<Result<Vec<_>>>()
    }

}
