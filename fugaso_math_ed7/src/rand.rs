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
    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)>;

    fn rand_cols_group(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> Result<(Vec<usize>, Vec<Vec<char>>)>;

    fn rand_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<Vec<i32>>>;
    
    fn rand_lifts_new(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<LiftItem>>;

    fn rand_cols(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> (Vec<usize>, Vec<Vec<char>>);

    fn rand_over(&mut self, grid: &Vec<Vec<char>>) -> Result<Option<Vec<Vec<char>>>>;
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
    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let reels = &self.p.reels_cfg[category];

        let stops_grid = reels
            .iter()
            .enumerate()
            .map(|(_c, dist)| {
                let mut m = dist
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_k, v))| {
                        if v.iter().any(|s| mega_thunder::is_spetials(*s)) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                let idx = self.p.base.rand.rand_vec_remove(&mut m)?;

                dist.iter()
                    .enumerate()
                    .find_map(|(i, (_k, v))| {
                        if i == idx {
                            Some((idx, v.clone()))
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| err_on!("random find item error!"))
            })
            .collect::<Result<Vec<_>>>()?;
        let (stops, grid) = (
            stops_grid.iter().map(|p| p.0).collect::<Vec<_>>(),
            stops_grid.into_iter().map(|p| p.1).collect::<Vec<_>>(),
        );
        Ok((stops, grid))
    }

    fn rand_cols_group(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let (stops, grid) = self.p.rand_cols_group(category, combos)?;
        
        Ok((stops, grid))
    }

    fn rand_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<Vec<i32>>> {
        let dist = &self.p.base.config.dist_coin[counter_idx];
        grid.iter().map(|c| {
            c.iter().map(|s| {
                    if *s == mega_thunder::SYM_SPETIALS[0] {
                        self.p.base.rand.rand_value(&dist)
                    } else {
                        self.p.base.config.map_jack.get(s).map_or(Ok(0), |m| Ok(*m))
                    }
                }).collect::<Result<Vec<_>>>()
        }).collect::<Result<Vec<_>>>()
    }

    fn rand_lifts_new(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Vec<LiftItem>> {
        let dist_coin = &self.p.base.config.dist_coin[counter_idx];
        let dist_mult = &self.p.base.config.dist_mult[counter_idx];
        grid.iter().enumerate().flat_map(|(col_idx, col)| {
            col.iter().enumerate().filter_map(move |(row_idx, symbol)| {
                if *symbol == mega_thunder::SYM_SPETIALS[2] {
                    Some((col_idx, row_idx))
                } else {
                    None
                }
            })
        }).map(|(col, row)| {
            Ok(LiftItem {
                p: (col, row),
                m: self.p.base.rand.rand_value(&dist_mult)?,
                v: self.p.base.rand.rand_value(&dist_coin)?,
            })
        }).collect::<Result<Vec<_>>>()
    }

    fn rand_cols(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> (Vec<usize>, Vec<Vec<char>>) {
        self.p.rand_cols(category, combos)
    }

    fn rand_over(&mut self, grid: &Vec<Vec<char>>) -> Result<Option<Vec<Vec<char>>>> {
        let r = self.p.base.rand.rand_value(&self.p.base.config.dist_over)?;
        if r > 0 {
            let mut over = grid.clone();
            let mut positions = over
                .iter()
                .enumerate()
                .flat_map(|(c, col)| {
                    col.iter().enumerate().filter_map(move |(r, s)| {
                        if *s != mega_thunder::SYM_COIN || !mega_thunder::is_spetials(*s) {
                            Some((c, r))
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>();

            let all = over
                .iter()
                .flat_map(|r| {
                    r.iter().filter(|c| {
                        **c == mega_thunder::SYM_COIN || mega_thunder::is_spetials(**c)
                    })
                })
                .count();
            debug!("all: {all} - {over:?}");
            if r > all {
                let mut remain = r - all;

                if positions.len() < remain {
                    return Err(err_on!("positions array is illegal!"));
                }
                while remain > 0 {
                    let (col, row) = self.p.base.rand.rand_vec_remove(&mut positions)?;
                    if col == 1 {
                        over[col][row] = mega_thunder::SYM_COIN;
                    } else {
                        over[col][row] = mega_thunder::SYM_SPETIALS[0];
                    }
                    remain -= 1;
                }
            }

            Ok(Some(over))
        } else {
            Ok(None)
        }
    }
}
