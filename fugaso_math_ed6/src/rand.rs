use std::sync::Arc;

use essential_rand::{err_on, error::RandError, random::RandomGenerator};
use fugaso_math::{
    config::{BaseConfig, ReelDist},
    rand::{BaseRandom, GroupRandom, ReelRandom, Result},
};
use log::{debug, error};
use mockall::*;

use crate::{
    config::{bonanza_1000, thunder_express, BonanzaLinkCashConfig, ThunderExpressConfig},
    protocol::{OverBonus, OverKind},
};

#[automock]
pub trait ThunderExpressRand {
    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)>;

    fn rand_cols_group(&mut self, category: usize, combos: Option<Vec<usize>>) -> Result<(Vec<usize>, Vec<Vec<char>>)>;

    fn rand_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize, respin: bool) -> Result<Vec<Vec<i32>>>;

    fn rand_cols(&mut self, category: usize, combos: Option<Vec<usize>>) -> (Vec<usize>, Vec<Vec<char>>);

    fn rand_over(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Option<Vec<Vec<char>>>>;
}

pub struct ThunderExpressRandom {
    pub p: GroupRandom<ThunderExpressConfig>,
}

impl ThunderExpressRandom {
    pub fn new(config: Arc<ThunderExpressConfig>, reels_cfg: Arc<ReelDist>) -> Self {
        Self {
            p: GroupRandom {
                reels_cfg,
                base: BaseRandom {
                    rand: RandomGenerator::new(),
                    rows: thunder_express::ROWS,
                    config,
                },
            },
        }
    }
}

impl ThunderExpressRand for ThunderExpressRandom {
    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let (stops, mut grid) = self.p.rand_cols_group(category, None)?;
        const CENTER_COL: usize = 2;
        for r in 0..grid[CENTER_COL].len() {
            grid[CENTER_COL][r] = thunder_express::SYM_COLLECT;
        }
        Ok((stops, grid))
    }

    fn rand_cols_group(&mut self, category: usize, combos: Option<Vec<usize>>) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let (stops, grid) = self.p.rand_cols_group(category, combos)?;
        Ok((stops, grid))
    }

    fn rand_mults(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize, respin: bool) -> Result<Vec<Vec<i32>>> {
        let category = counter_idx * thunder_express::NUM_CATEGORIES + respin as usize;
        let dist = &self.p.base.config.dist_coin[category];
        grid.iter()
            .map(|c| {
                c.iter()
                    .map(|s| {
                        if *s == thunder_express::SYM_COINS[0] {
                            self.p.base.rand.rand_value(&dist)
                        } else {
                            Ok(0)
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()
    }

    fn rand_cols(&mut self, category: usize, combos: Option<Vec<usize>>) -> (Vec<usize>, Vec<Vec<char>>) {
        self.p.rand_cols(category, combos)
    }

    fn rand_over(&mut self, grid: &Vec<Vec<char>>, counter_idx: usize) -> Result<Option<Vec<Vec<char>>>> {
        let r = self.p.base.rand.rand_value(&self.p.base.config.dist_over[counter_idx])?;
        if r > 0 {
            let mut over = grid.clone();
            const CENTER_COL: usize = 2;
            let mut positions = over
                .iter()
                .enumerate()
                .filter(|(c, _)| *c != CENTER_COL)
                .flat_map(|(c, col)| {
                    col.iter().enumerate().filter_map(move |(r, s)| {
                        if *s != thunder_express::SYM_COLLECT && !thunder_express::is_coin(*s) {
                            Some((c, r))
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>();

            let coins = over.iter().flat_map(|r| r.iter().filter(|c| thunder_express::is_coin(**c))).count();
            let collects = over.iter().flat_map(|r| r.iter().filter(|c| **c == thunder_express::SYM_COLLECT)).count();
            debug!("all: {coins} + {collects}");
            if r > coins + collects {
                if collects == 0 {
                    let r = self.p.base.rand.random(0, over[CENTER_COL].len());
                    over[CENTER_COL][r] = thunder_express::SYM_COLLECT;
                }
                let mut remain = r - collects - coins;

                if positions.len() < remain {
                    return Err(err_on!("positions array is illegal!"));
                }
                while remain > 0 {
                    let (col, row) = self.p.base.rand.rand_vec_remove(&mut positions)?;
                    over[col][row] = thunder_express::SYM_COINS[0];
                    remain -= 1;
                }
            }

            Ok(Some(over))
        } else {
            Ok(None)
        }
    }
}

#[automock]
pub trait BonanzaLink1000Rand {
    fn rand_mults(&mut self, grid: &Vec<Vec<char>>) -> Result<Vec<Vec<i32>>>;

    fn rand_cols(&mut self, category: usize, combos: Option<Vec<usize>>) -> (Vec<usize>, Vec<Vec<char>>);

    fn rand_respin_cols(&mut self, category: usize, prev_grid: &Vec<Vec<char>>, prev_stops: &Vec<usize>) -> (Vec<usize>, Vec<Vec<char>>);

    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)>;

    fn rand_over_coins(&mut self, category: usize, grid: &Vec<Vec<char>>) -> Result<Option<OverBonus>>;

    fn rand_pull(&mut self, category: usize, grid: &Vec<Vec<char>>, code_on: Vec<char>, code_off: char) -> Result<Option<OverBonus>>;
}

pub struct BonanzaLink1000Random {
    pub p: BaseRandom<BonanzaLinkCashConfig>,
}

impl BonanzaLink1000Random {
    pub fn new(config: Arc<BonanzaLinkCashConfig>) -> Self {
        Self {
            p: BaseRandom {
                rand: RandomGenerator::new(),
                rows: 3,
                config,
            },
        }
    }

    fn rand_over_shoot(&mut self, category: usize, grid: &Vec<Vec<char>>) -> Result<OverBonus> {
        let (_, mut grid_new) = self.rand_cols(category, None);
        for c in 0..grid.len() {
            for r in 0..grid[c].len() {
                if bonanza_1000::SYM_WILD == grid[c][r] {
                    grid_new[c][r] = grid[c][r];
                } else if bonanza_1000::SYM_WILD == grid_new[c][r] {
                    grid_new[c][r] = bonanza_1000::SYM_COIN;
                }
            }
        }
        let coins = grid_new.iter().flat_map(|c| c.iter().filter(|c| **c == bonanza_1000::SYM_COIN)).count();
        let grid_on = if coins == 0 {
            let over = self.rand_over_bang(&grid_new)?;
            over.grid
        } else {
            grid_new
        };
        Ok(OverBonus {
            kind: OverKind::Shoot,
            grid: grid_on,
        })
    }

    fn rand_over_bang(&mut self, grid: &Vec<Vec<char>>) -> Result<OverBonus> {
        let mut positions = (0..grid.len())
            .flat_map(|c| {
                (0..grid[c].len()).filter_map(move |r| {
                    if bonanza_1000::SYM_WILD == grid[c][r] {
                        None
                    } else {
                        Some((c, r))
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut len = std::cmp::min(positions.len(), self.p.rand.rand_value(&self.p.config.dist_bang)?);
        let mut grid_new = grid.clone();
        while len > 0 {
            let p = self.p.rand.rand_vec_remove(&mut positions)?;
            grid_new[p.0][p.1] = bonanza_1000::SYM_COIN;
            len -= 1;
        }
        Ok(OverBonus {
            kind: OverKind::Bang,
            grid: grid_new,
        })
    }
}

impl BonanzaLink1000Rand for BonanzaLink1000Random {
    fn rand_mults(&mut self, grid: &Vec<Vec<char>>) -> Result<Vec<Vec<i32>>> {
        grid.iter()
            .map(|c| {
                c.iter()
                    .map(|v| {
                        if *v == bonanza_1000::SYM_COIN {
                            self.p.rand.rand_value(&self.p.config.dist_coin)
                        } else {
                            Ok(0)
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()
    }

    fn rand_cols(&mut self, category: usize, combos: Option<Vec<usize>>) -> (Vec<usize>, Vec<Vec<char>>) {
        self.p.rand_cols(category, combos)
    }

    fn rand_respin_cols(&mut self, category: usize, prev_grid: &Vec<Vec<char>>, prev_stops: &Vec<usize>) -> (Vec<usize>, Vec<Vec<char>>) {
        let (mut stops, mut grid) = self.p.rand_cols(category, None);
        let reels = &self.p.config.reels;
        let reels_on = &reels[category];

        for c in 0..stops.len() {
            if prev_grid[c].contains(&bonanza_1000::SYM_SCAT) {
                let reel = &reels_on[c];
                stops[c] = if prev_stops[c] > 0 {
                    prev_stops[c] - 1
                } else {
                    reel.len() - 1
                };
                grid[c] = (0..self.p.rows).map(|r| reel[(stops[c] + r) % reel.len()]).collect::<Vec<_>>()
            }
        }
        (stops, grid)
    }

    fn rand_buy_cols(&mut self, category: usize) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        let reels = self.p.config.reels();
        let reels_on = &reels[category];

        let mut columns = (0..reels_on.len()).collect::<Vec<_>>();
        let mut inc = vec![0_usize; bonanza_1000::SCATTERS_FOR_FREE];
        for i in 0..inc.len() {
            let c = self.p.rand.rand_vec_remove(&mut columns)?;
            inc[i] = c;
        }
        let stops = reels_on
            .iter()
            .enumerate()
            .map(|(c, col)| {
                if inc.contains(&c) {
                    let positions = col
                        .iter()
                        .enumerate()
                        .filter_map(|(r, v)| {
                            if *v == bonanza_1000::SYM_SCAT {
                                Some(r)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    let position = self.p.rand.rand_vec(&positions)? as i32;
                    let row = self.p.rand.random(0, self.p.rows) as i32;
                    let rand_pos = (position - row + col.len() as i32) % col.len() as i32;
                    Ok(rand_pos as usize)
                } else {
                    Ok(self.p.rand.random(0, col.len()))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        let grid = reels_on
            .iter()
            .enumerate()
            .map(|(p, reel)| {
                let s = stops[p];
                (0..self.p.rows).map(|r| reel[(s + r) % reel.len()]).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Ok((stops, grid))
    }

    fn rand_over_coins(&mut self, category: usize, grid: &Vec<Vec<char>>) -> Result<Option<OverBonus>> {
        let kind = self.p.rand.rand_value(&self.p.config.dist_over);
        kind.and_then(|k| match k {
            OverKind::Bang => self.rand_over_bang(grid).map(|o| Some(o)),
            OverKind::Shoot => self.rand_over_shoot(category, grid).map(|o| Some(o)),
            _ => Err(err_on!("illegal over bonus!")),
        })
    }

    fn rand_pull(&mut self, category: usize, grid: &Vec<Vec<char>>, code_on: Vec<char>, code_off: char) -> Result<Option<OverBonus>> {
        let r = self.p.rand.random(0, self.p.config.dist_pull.1);
        if r < self.p.config.dist_pull.0 {
            let reels = self.p.config.reels();
            let reels_on = &reels[category];
            let cols = grid
                .iter()
                .enumerate()
                .filter_map(|(c, col)| {
                    if col.contains(&code_off) {
                        None
                    } else {
                        Some(c)
                    }
                })
                .collect::<Vec<_>>();
            if cols.is_empty() {
                return Ok(None);
            }
            let column = self.p.rand.rand_vec(&cols)?;
            let positions = reels_on[column]
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    if code_on.contains(v) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if positions.is_empty() {
                error!("no positions for code={code_on:?} column={column} category={category}");
                return Err(err_on!("positions is empty!"));
            }
            let position = self.p.rand.rand_vec(&positions)? as i32;
            let row = self.p.rand.random(0, self.p.rows) as i32;
            let rand_pos = (position - row + reels_on[column].len() as i32) % reels_on[column].len() as i32;
            let mut grid_new = grid.clone();
            grid_new[column] = (0..self.p.rows)
                .map(|r| {
                    let reel = &reels_on[column];
                    reel[(rand_pos as usize + r) % reel.len()]
                })
                .collect();

            Ok(Some(OverBonus {
                kind: OverKind::Pull,
                grid: grid_new,
            }))
        } else {
            Ok(None)
        }
    }
}
