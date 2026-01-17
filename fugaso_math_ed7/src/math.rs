use crate::config::{mega_thunder, MegaThunderConfig};
use crate::protocol::MegaThunderInfo;
use crate::rand::{MegaThunderRand, MegaThunderRandom};
use essential_core::err_on;
use essential_core::error::ServerError;
use fugaso_data::fugaso_action;
use fugaso_data::fugaso_action::ActionKind;
use fugaso_math::fsm::SlotFSM;
use fugaso_math::math::{
    BetCalculator, BetDenomCounterCalculator, GameInitArg, JoinArg, MathSettings, Request,
    SlotMath, SpinArg, Step,
};
use fugaso_math::protocol::{id, GameData, GameResult, InitialData, StartInfo};
use fugaso_math::protocol::{Gain, SpinData, Win};
use fugaso_math::validator::RequestValidator;
use log::{debug, info};
use std::sync::Arc;
use std::{usize, vec};

pub struct MegaThunderMath<R: MegaThunderRand> {
    pub result: Arc<GameData<MegaThunderInfo, StartInfo>>,
    pub config: Arc<MegaThunderConfig>,
    pub rand: R,
}

impl MegaThunderMath<MegaThunderRandom> {
    pub fn new(config: Option<String>, reels_cfg: Option<String>) -> Result<Self, ServerError> {
        let cfg = config
            .map(|j| {
                serde_json::from_str(&j)
                    .map(|v| Arc::new(v))
                    .map_err(|e| err_on!(e))
            })
            .unwrap_or(Ok(Arc::clone(&mega_thunder::CFG)))?;
        let reels_cfg_on = reels_cfg
            .map(|j| {
                serde_json::from_str(&j)
                    .map(|v| Arc::new(v))
                    .map_err(|e| err_on!(e))
            })
            .unwrap_or(Ok(Arc::clone(&mega_thunder::REELS_CFG)))?;
        let rand = MegaThunderRandom::new(Arc::clone(&cfg), reels_cfg_on);
        Self::custom(rand, cfg)
    }
}

impl<R: MegaThunderRand> MegaThunderMath<R> {

    pub fn configured(rand: R) -> Result<Self, ServerError> {
        Self::custom(rand, Arc::clone(&mega_thunder::CFG))
    }

    pub fn custom(mut rand: R, config: Arc<MegaThunderConfig>) -> Result<Self, ServerError> {
        let category = mega_thunder::BASE_CATEGORY;
        let (stops, grid) = rand.rand_spin_grid(category, None)?;
            let mut mults = vec![vec![0; 3]; 5];
            if let Some(m) = rand.rand_coins_values(&grid, &mults, category) {mults = m};
        //let mults = rand.rand_coins_values(&grid, 0)?;
        let special = if mults.len() > 0 {
            Some(MegaThunderInfo {
                mults,
                ..Default::default()
            })
        } else {
            None
        };
        let m = Self {
            rand,
            result: Arc::new(GameData::Spin(SpinData {
                id: id::GAME_DATA,
                result: GameResult {
                    stops,
                    holds: grid.iter().flat_map(|r| r.iter().map(|_| 0)).collect(),
                    grid,
                    special,
                    ..Default::default()
                },
                ..Default::default()
            })),
            config,
        };
        Ok(m)
    }

    pub fn check_lines(&mut self, req: &Request, counter_idx: usize, round_mul: i32, grid: &Vec<Vec<char>>, ) -> Result<(Vec<Gain>, Vec<i32>, MegaThunderInfo), ServerError> {
        let specials = grid.iter().flat_map(|c| c.iter().filter(|v| mega_thunder::is_specials(**v))).count();
        let overlay = if specials >= 2 && specials <= 3 {self.rand.rand_spin_over(grid)?} else {None};
        debug!("over: {overlay:?}");
        let grid_on = overlay.as_ref().unwrap_or(grid);

        let lines = &self.config.lines;
        let combs = &self.config.wins;
        let mut gains = lines.iter().enumerate().filter_map(|(line_num, l)| {
            let mut w = grid[0][l[0]];
            let mut symbols = 0;

            for j in 0..l.len() {
                let ch = grid[j][l[j]];
                if w == mega_thunder::SYM_WILD {
                    w = ch
                }
                if w == ch || ch == mega_thunder::SYM_WILD {
                    symbols += 1;
                } else {
                    break;
                }
            }

            let factor = *combs.get(&w).and_then(|m| m.get(&symbols)).unwrap_or(&0);
            if factor > 0 {
                let amount = factor as i64 * req.bet as i64 * round_mul as i64;
                Some(Gain {
                    symbol: w,
                    count: symbols,
                    amount,
                    line_num,
                    multi: 1,
                    ..Default::default()
                })
            } else {
                None
            }
        }).collect::<Vec<_>>();
        
        let coins = grid_on.iter().flat_map(|c| c.iter().filter(|v| {**v == mega_thunder::SYM_COIN || **v == mega_thunder::SYM_JACKPOT})).count();
        let mutipliers = grid_on.iter().flat_map(|c| c.iter().filter(|v| {**v == mega_thunder::SYM_MULTI})).count();
        debug!("coins: {coins} mutipliers: {mutipliers}");
        
        let (mut respins, grand, accum, mults, lifts, lifts_new, mut total, remain_coins_win, remain_gain) = 
        if coins + mutipliers >= 6 {

            let mut mults = vec![vec![0; 3]; 5];
            if let Some(m) = self.rand.rand_coins_values(grid_on, &mults, counter_idx) {mults = m};
            if let Some(m) = self.rand.rand_jackpots_values(grid_on, &mults, counter_idx) {mults = m};
            debug!("mults: {mults:?}");

            let mut lifts_new = self.rand.rand_lifts_values_mults(grid_on, counter_idx)?;
            debug!("lifts_new: {lifts_new:?}");

            let mut lifts = vec![vec![0; 3]; 5];
            grid_on.iter().enumerate().for_each(|(col_num, col)| {
                col.iter().enumerate().for_each(|(row_num, row)| {
                    if *row == mega_thunder::SYM_COIN || *row == mega_thunder::SYM_JACKPOT {
                        lifts[col_num][row_num] = 1;
                    }
                });
            });
            lifts_new.iter_mut().for_each(|lift| {
                lifts.iter_mut().for_each(|lc| {
                    lc.iter_mut().for_each(|lr| {
                        *lr *= lift.m;
                    });
                });
                mults[lift.p.0][lift.p.1] = lift.v;
                lifts[lift.p.0][lift.p.1] = 1;
            });
            debug!("lifts: {lifts:?}");

            let mut grand = vec![0; grid.len()];
            let coins_win = mults.iter().enumerate().map(|(col_num, col)| {
                if col.iter().all(|v| *v > 0) {
                    grand[col_num] += 1;
                    let coins_win = col.iter().enumerate().map(|(row_num, row)| {
                        row * lifts[col_num][row_num] * req.bet * req.denom
                    }).sum::<i32>();
                    gains.push(Gain { 
                        symbol: mega_thunder::SYM_COIN_COLUMN, 
                        count: 3, 
                        amount: coins_win as i64, 
                        line_num: col_num, 
                        multi: 1, 
                        ..Default::default()
                    });
                    coins_win
                } else {0}
            }).sum::<i32>();

            let mut remain_coins_count = 0;
            let remain_coins_win = mults.iter().enumerate().map(|(col_num, col)| {
                if !col.iter().all(|v| *v > 0) && col.iter().any(|v| *v > 0) {
                    col.iter().enumerate().map(|(row_num, row)| {
                        remain_coins_count += 1;
                        row * lifts[col_num][row_num] * req.bet * req.denom
                    }).sum::<i32>()
                } else {0}
            }).sum::<i32>();
            let remain_gain = if remain_coins_count > 0 {
                Some(Gain { 
                    symbol: mega_thunder::SYM_COIN, 
                    count: remain_coins_count, 
                    amount: remain_coins_win as i64, 
                    line_num: 0, 
                    multi: 1, 
                    ..Default::default()
                })
            } else {None};
            debug!("grand: {grand:?}");

            let total = gains.iter().map(|w| w.amount).sum::<i64>();
            let respins = mega_thunder::BONUS_COUNT;
            let accum = coins_win as i64;
            (respins, grand, accum, mults, lifts, lifts_new, total, remain_coins_win, remain_gain)
        } else {
            let have_coin = coins > 0;
            let have_mutiplier = mutipliers > 0;

            let mut mults = vec![vec![0; 3]; 5];
            if let Some(m) = self.rand.rand_coins_values(grid_on, &mults, counter_idx) {mults = m};
            if let Some(m) = self.rand.rand_jackpots_values(grid_on, &mults, counter_idx) {mults = m};
            debug!("mults: {mults:?}");

            let mut lifts_new = self.rand.rand_lifts_values_mults(grid_on, counter_idx)?;
            debug!("lifts_new: {lifts_new:?}");

            let mut lifts = vec![vec![0; 3]; 5];
            grid_on.iter().enumerate().for_each(|(col_num, col)| {
                col.iter().enumerate().for_each(|(row_num, row)| {
                    if *row == mega_thunder::SYM_COIN || *row == mega_thunder::SYM_JACKPOT {
                        lifts[col_num][row_num] = 1;
                    }
                });
            });
            lifts_new.iter_mut().for_each(|lift| {
                lifts.iter_mut().for_each(|lc| {
                    lc.iter_mut().for_each(|lr| {
                        *lr *= lift.m;
                    });
                });
                lifts[lift.p.0][lift.p.1] = lift.m;
                lift.v = mults.iter().flat_map(|row| row.iter()).sum::<i32>();
            });
            debug!("lifts: {lifts:?}");
            
            if have_coin && have_mutiplier {
                let coins_win = mults.iter().enumerate().map(|(col_num, col)| {
                    col.iter().enumerate().map(|(row_num, row)| {
                        row * lifts[col_num][row_num] * req.bet * req.denom
                    }).sum::<i32>()
                }).sum::<i32>();
                gains.push(Gain { 
                    symbol: mega_thunder::SYM_COIN, 
                    count: coins, 
                    amount: coins_win as i64, 
                    line_num: 0, 
                    multi: 1, 
                    ..Default::default()
                });
            };
            let remain_coins_win = 0;
            let remain_gain = None;

            let total = gains.iter().map(|w| w.amount).sum::<i64>();
            let grand = vec![];
            let respins = 0;
            let accum = 0;
            if mults.iter().flatten().all(|&v| v == 0) {mults.clear()};
            if lifts.iter().flatten().all(|&v| v == 0) {lifts.clear()};
            (respins, grand, accum, mults, lifts, lifts_new, total, remain_coins_win, remain_gain)
        };

        let max = self.calc_max_win(req);
        let stop = if total + remain_coins_win as i64 >= max {
            if let Some(g) = remain_gain {gains.push(g);}
            respins = 0;
            total = max;
            Some(self.config.stop_factor)
        } else {None};
        let special = MegaThunderInfo {
            mults,
            lifts,
            lifts_new,
            grand,
            respins,
            overlay,
            total,
            accum,
            stop,
            ..Default::default()
        };
        debug!("{special:?}");
        Ok((gains, vec![0], special))
    }

    pub fn check_bonus(&mut self, req: &Request, counter_idx: usize, _multiplier: i32, grid: &mut Vec<Vec<char>>, prev_grid: &Vec<Vec<char>>, prev_info: &MegaThunderInfo, prev_total: i64, ) -> Result<(Vec<Gain>, MegaThunderInfo, Vec<i32>), ServerError> {
        let prev_specials = prev_grid.iter().flat_map(|c| c.iter().filter(|v| {mega_thunder::is_specials(**v)})).count();
        debug!("prev_specials: {prev_specials:?}");
        let specials = grid.iter().flat_map(|c| c.iter().filter(|v| {mega_thunder::is_specials(**v)})).count();
        debug!("specials: {specials:?}");

        let mut mults = vec![vec![0; 3]; 5];
        if let Some(m) = self.rand.rand_coins_values(&grid, &mults, counter_idx) {mults = m};
        if let Some(m) = self.rand.rand_jackpots_values(&grid, &mults, counter_idx) {mults = m};
        for c in 0..mults.len() {
            for r in 0..mults[c].len() {
                if prev_info.mults[c][r] > 0 {
                    mults[c][r] = prev_info.mults[c][r];
                }
            }
        }
        debug!("mults: {mults:?}");

        let mut lifts_new = self.rand.rand_lifts_values_mults(grid, counter_idx)?;
        debug!("lifts_new: {lifts_new:?}");

        let mut lifts = vec![vec![0; 3]; 5];
        for c in 0..lifts.len() {
            for r in 0..lifts[c].len() {
                if prev_info.lifts[c][r] > 0 {
                    lifts[c][r] = prev_info.lifts[c][r];
                }
            }
        }
        grid.iter().enumerate().for_each(|(col_num, col)| {
            col.iter().enumerate().for_each(|(row_num, row)| {
                if *row == mega_thunder::SYM_COIN || *row == mega_thunder::SYM_JACKPOT {
                    if lifts[col_num][row_num] == 0 {lifts[col_num][row_num] = 1};
                }
            });
        });
        lifts_new.iter_mut().for_each(|lift| {
            lifts.iter_mut().for_each(|lc| {
                lc.iter_mut().for_each(|lr| {
                    *lr *= lift.m;
                });
            });
            mults[lift.p.0][lift.p.1] = lift.v;
            lifts[lift.p.0][lift.p.1] = 1;
        });
        debug!("lifts: {lifts:?}");

        let mut respins = if specials > prev_specials {mega_thunder::BONUS_COUNT} else {prev_info.respins - 1};
        let (grand, mut gains, remain_coins_win, remain_gain) = if respins > 0 {
            let mut gains = vec![];
            let mut grand = prev_info.grand.clone();
            mults.iter().enumerate().for_each(|(col_num, col)| {
                if col.iter().all(|v| *v > 0) {
                    grand[col_num] += 1;
                    let  coins_win = col.iter().enumerate().map(|(row_num, row)| {
                        row * lifts[col_num][row_num] * req.bet * req.denom
                    }).sum::<i32>();
                    gains.push(Gain { 
                        symbol: mega_thunder::SYM_COIN_COLUMN, 
                        count: 3, 
                        amount: coins_win as i64, 
                        line_num: col_num, 
                        multi: 1, 
                        ..Default::default()
                    });
                }
            });
            let mut remain_coins_count = 0;
            let remain_coins_win = mults.iter().enumerate().map(|(col_num, col)| {
                if !col.iter().all(|v| *v > 0) && col.iter().any(|v| *v > 0) {
                    col.iter().enumerate().map(|(row_num, &row)| {
                        remain_coins_count += 1;
                        row * lifts[col_num][row_num] * req.bet * req.denom
                    }).sum::<i32>()
                } else {0}
            }).sum::<i32>();
            let remain_gain = if remain_coins_count > 0 {
                Some(Gain { 
                    symbol: mega_thunder::SYM_COIN, 
                    count: remain_coins_count, 
                    amount: remain_coins_win as i64, 
                    line_num: 0, 
                    multi: 1, 
                    ..Default::default()
                })
            } else {None};
            if prev_info.grand.iter().any(|v| {*v == 0}) && grand.iter().all(|v| {*v > 0}) {
                let jp_amount = self.config.grand_jackpot * req.bet * req.denom;
                gains.push(Gain { 
                    symbol: mega_thunder::SYM_GRAND_JACKPOT, 
                    count: 1, 
                    amount: jp_amount as i64, 
                    line_num: 0, 
                    multi: 1, 
                    ..Default::default()
                });
            };
            (grand, gains, remain_coins_win, remain_gain)
        } else {
            let mut gains = vec![];
            let grand = prev_info.grand.clone();
            let mut remain_coins_count = 0;
            let coins_win = mults.iter().enumerate().map(|(col_num, col)| {
                col.iter().enumerate().map(|(row_num, &row)| {
                    if row > 0 {remain_coins_count += 1};
                    row * lifts[col_num][row_num] * req.bet * req.denom
                }).sum::<i32>()
            }).sum::<i32>();
            if remain_coins_count > 0 {
                gains.push(Gain { 
                    symbol: mega_thunder::SYM_COIN, 
                    count: remain_coins_count, 
                    amount: coins_win as i64, 
                    line_num: 0, 
                    multi: 1, 
                    ..Default::default()
                });
            };
            let remain_coins_win = 0;
            let remain_gain = None;
            (grand, gains, remain_coins_win, remain_gain)
        };
        debug!("grand: {grand:?}");

        let max = self.calc_max_win(req);
        let stop = if prev_total + remain_coins_win as i64 >= max {
            if let Some(g) = remain_gain {gains.push(g);}
            respins = 0;
            Some(self.config.stop_factor)
        } else {None};
        let sum = gains.iter().map(|g| g.amount).sum::<i64>();
        let total = std::cmp::min(max, prev_total + sum);
        let accum = std::cmp::min(max, prev_info.accum + sum);
        Ok((
            gains,
            MegaThunderInfo {
                mults,
                lifts,
                lifts_new,
                grand: grand.clone(),
                respins,
                total,
                accum,
                stop,
                ..Default::default()
            },
            vec![0],
        ))
    }

    fn calc_max_win(&self, req: &Request) -> i64 {
        let calculator = self.create_bet_calculator();
        let playing_bet = calculator.calc_playing_bet(&req);
        let max = playing_bet * self.config.stop_factor as i64;
        max
    }

    fn apply_prev(&self, current: &mut Vec<Vec<char>>, prev: &Vec<Vec<char>>) {
        for c in 0..prev.len() {
            for r in 0..prev[c].len() {
                if mega_thunder::is_specials(prev[c][r]) {
                    current[c][r] = prev[c][r]
                }
            }
        }
    }
}

impl<R: MegaThunderRand> SlotMath for MegaThunderMath<R> {
    type Special = MegaThunderInfo;
    type Calculator = BetDenomCounterCalculator;
    type Restore = StartInfo;
    type PlayFSM = SlotFSM;
    type Rand = R;
    type Input = Request;
    type V = RequestValidator;

    fn join(&self, arg: JoinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let result = match self.result.as_ref() {
            GameData::Initial(v) => v.result.clone(),
            GameData::Spin(v) | GameData::ReSpin(v) => v.result.clone(),
            GameData::Collect(v) => v.result.clone(),
            _ => return Err(err_on!("Illegal state!")),
        };
        let mut wins = self.config.wins.iter().flat_map(|p| {
            p.1.iter().map(|w| Win {
                symbol: *p.0,
                count: *w.0,
                factor: *w.1,
            })
        }).collect::<Vec<Win>>();
        wins.sort_by(|a, b| {a.symbol.cmp(&b.symbol).then(a.count.cmp(&b.count)).then(a.factor.cmp(&b.factor))});

        Ok(GameData::Initial(InitialData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            min_bet: 0,
            max_bet: 0,
            lines: self.config.lines.clone(),
            reels: self.config.reels.clone(),
            wins,
            category: 0,
            result,
            poss_lines: arg.poss_lines,
            poss_bets: arg.poss_bets,
            poss_denom: arg.poss_denom,
            poss_reels: arg.poss_reels,
            poss_bet_counters: arg.poss_bet_counters,
            curr_lines: arg.curr_lines,
            curr_bet: arg.curr_bet,
            curr_denom: arg.curr_denom,
            curr_reels: arg.curr_reels,
            bet_counter: arg.bet_counter,
            next_act: arg.next_act,
            round_id: arg.round_id,
            round_type: arg.round_type,
            round_multiplier: arg.round_multiplier,
            promo: arg.promo,
            free: None,
        }))
    }

    fn init(
        &mut self,
        arg: GameInitArg,
        actions: &Vec<fugaso_action::Model>,
    ) -> Result<(), ServerError> {
        if actions.is_empty() {
            return Ok(());
        }
        let action = &actions[actions.len() - 1];
        if let Some(next) = &action.next_act {
            let result0: GameResult<Self::Special, Self::Restore> =
                GameResult::from_action(&actions[0])?;
            let mut result_on = GameResult::from_action(action)?;

            let restore = match result0.special.as_ref() {
                Some(MegaThunderInfo {
                    mults,
                    overlay: Some(o),
                    ..
                }) => Some(StartInfo {
                    mults: mults.clone(),
                    grid: Some(o.clone()),
                    ..Default::default()
                }),
                Some(MegaThunderInfo { mults, .. }) => Some(StartInfo {
                    mults: mults.clone(),
                    grid: Some(result0.grid.clone()),
                    ..Default::default()
                }),
                None => None,
            };
            if *next == ActionKind::BET {
                result_on.stops = result0.stops;
                result_on.grid = result0.grid;
                result_on.special = result0.special;
                result_on.holds = result0.holds;
            }
            if action.act_descr == Some(ActionKind::BET) {
                result_on.total = result_on
                    .special
                    .as_ref()
                    .map(|s| s.total)
                    .unwrap_or(result_on.total);
            }
            result_on.restore = restore;
            let spin_data = SpinData {
                id: id::GAME_DATA,
                balance: 0,
                credit_type: 100,
                result: result_on,
                next_act: next.clone(),
                category: action.reel_combo as usize,
                curr_lines: arg.curr_lines,
                curr_bet: arg.curr_bet,
                curr_denom: arg.curr_denom,
                curr_reels: arg.curr_reels,
                round_id: arg.round_id,
                round_type: arg.round_type,
                round_multiplier: arg.round_multiplier,
                promo: arg.promo,
                free: None,
            };
            match action.act_descr.as_ref() {
                None => {}
                Some(a) => match a {
                    ActionKind::RESPIN => self.result = Arc::new(GameData::ReSpin(spin_data)),
                    _ => self.result = Arc::new(GameData::Spin(spin_data)),
                },
            }
        }
        Ok(())
    }

    fn spin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>, ) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let (category, stops, grid) = if request.bet_counter == self.config.bet_counters[0] {
            let category = mega_thunder::BASE_CATEGORY;
            let (stops, mut grid) = self.rand.rand_spin_grid(category, combo)?;
            if let Some(g) = self.rand.rand_grid_coins(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_jackpots(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_lifts(&grid) {grid = g};
            (category, stops, grid)
        } else if request.bet_counter == self.config.bet_counters[1] {
            let category = mega_thunder::BASE_CATEGORY + 1;
            let (stops, mut grid) = self.rand.rand_buy_spin_grid(category)?;
            if let Some(g) = self.rand.rand_grid_coins(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_jackpots(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_lifts(&grid) {grid = g};
            (category, stops, grid)
        } else if request.bet_counter == self.config.bet_counters[2] {
            let category = mega_thunder::BASE_CATEGORY + 2;
            let (stops, mut grid) = self.rand.rand_buy_spin_grid(category)?;
            if let Some(g) = self.rand.rand_grid_coins(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_jackpots(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_lifts(&grid) {grid = g};
            if let Some(g) = self.rand.rand_grid_lifts(&grid) {grid = g};
            (category, stops, grid)
        } else {return Err(err_on!("illegal bet_counter!"));};
        let count_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet counter!"))?;
        let (gains, holds, special) = self.check_lines(request, count_idx, arg.round_multiplier, &grid)?;
        let total = special.total;
        let (next_act, restore) = if special.respins > 0 {
            let grid_on = match special.overlay.as_ref() {
                None => grid.clone(),
                Some(o) => o.clone(),
            };
            (
                ActionKind::RESPIN,
                Some(StartInfo {
                    grid: Some(grid_on),
                    mults: special.mults.clone(),
                    ..Default::default()
                }),
            )
        } else {
            (ActionKind::CLOSE, None)
        };
        let result = GameData::Spin(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance - arg.stake,
            credit_type: 100,
            result: GameResult {
                total,
                stops,
                holds,
                grid,
                special: Some(special),
                gains,
                restore,
                ..Default::default()
            },
            curr_lines: request.line,
            curr_bet: request.bet,
            curr_denom: request.denom,
            curr_reels: request.reels,
            next_act,
            category,
            round_id: arg.round_id,
            round_type: arg.round_type,
            round_multiplier: arg.round_multiplier,
            promo: arg.promo,
            ..Default::default()
        });
        Ok(result)
    }

    fn respin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>, ) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let prev = Arc::clone(&self.result);
        let (prev_total, mut prev_info, mut prev_grid, prev_restore) = match prev.as_ref() {
            GameData::Spin(v) => {
                let prev_info = v.result.special.as_ref().ok_or_else(|| err_on!("Illegal state!"))?.clone();
                let grid = if let Some(special) = &v.result.special {
                    if let Some(over) = special.overlay.as_ref() {over.clone()} else {v.result.grid.clone()}
                } else {
                    v.result.grid.clone()
                };
                (v.result.total, prev_info, grid, &v.result.restore)
            }
            GameData::ReSpin(v) => {
                let prev_info = v.result.special.as_ref().ok_or_else(|| err_on!("Illegal state!"))?.clone();
                let grid = v.result.grid.clone();
                (v.result.total, prev_info, grid, &v.result.restore)
            }
            _ => return Err(err_on!("Illegal state!")),
        };

        for col_num in 0..prev_info.mults.len() {
            if prev_info.mults[col_num].iter().all(|v| *v > 0) {
                for row_num in 0..prev_info.mults[col_num].len() {
                    prev_info.mults[col_num][row_num] = 0;
                    prev_info.lifts[col_num][row_num] = 0;
                    prev_grid[col_num][row_num] = '@';
                };
            };
        };

        let counter_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet_counter!"))?;
        let category = mega_thunder::BONUS_OFFSET + counter_idx;
        let (stops, mut grid) = self.rand.rand_respin_grid(category, combo);
        self.apply_prev(&mut grid, &prev_grid);
        debug!("{grid:?}");

        let (gains, special, holds) = self.check_bonus(request, counter_idx, arg.round_multiplier, &mut grid, &prev_grid, &prev_info, prev_total, )?;
        let total = special.total;
        let (next_act, restore, extra_data) = if special.respins > 0 {
            (ActionKind::RESPIN, prev_restore.clone(), None)
        } else {
            (ActionKind::CLOSE, None, prev_restore.clone())
        };
        let result = GameData::ReSpin(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            result: GameResult {
                total,
                stops,
                holds,
                grid,
                special: Some(special),
                gains,
                restore,
                extra_data,
                ..Default::default()
            },
            curr_lines: request.line,
            curr_bet: request.bet,
            curr_denom: request.denom,
            curr_reels: request.reels,
            next_act,
            category,
            round_id: arg.round_id,
            round_type: arg.round_type,
            round_multiplier: arg.round_multiplier,
            promo: arg.promo,
            ..Default::default()
        });
        Ok(result)
    }

    fn post_process(
        &mut self,
        kind: ActionKind,
        mut data: GameData<Self::Special, Self::Restore>,
    ) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError> {
        data.set_next_act(kind);
        self.result = Arc::new(data);
        Ok(self.result.clone())
    }

    fn close(
        &self,
        next_act: ActionKind,
    ) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        match self.result.as_ref() {
            GameData::Spin(v) => Ok(GameData::Spin(SpinData {
                next_act,
                ..v.clone()
            })),
            GameData::ReSpin(v) => Ok(GameData::ReSpin(SpinData {
                next_act,
                ..v.clone()
            })),
            _ => return Err(err_on!("Illegal state!")),
        }
    }

    fn collect(
        &self,
        request: &Request,
        arg: SpinArg,
    ) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let (category, result, free_game) = match self.result.as_ref() {
            GameData::Initial(v) => (v.category, v.result.clone(), v.free.clone()),
            GameData::Spin(v) | GameData::ReSpin(v) => {
                (v.category, v.result.clone(), v.free.clone())
            }
            GameData::Collect(v) => (v.category, v.result.clone(), v.free.clone()),
            _ => return Err(err_on!("Illegal state!")),
        };
        info!("collect: {}", result.total);
        Ok(GameData::Collect(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance + result.total,
            credit_type: 100,
            result,
            curr_lines: request.line,
            curr_bet: request.bet,
            curr_denom: request.denom,
            curr_reels: request.reels,
            next_act: arg.next_act,
            category,
            round_id: arg.round_id,
            round_type: arg.round_type,
            round_multiplier: arg.round_multiplier,
            promo: arg.promo,
            free: free_game,
        }))
    }

    fn settings(&self) -> MathSettings {
        MathSettings {
            lines: vec![self.config.lines.len()],
            reels: vec![self.config.reels[0].len()],
            bet_counters: self.config.bet_counters.clone(),
        }
    }

    fn set_rand(&mut self, rand: Self::Rand) {
        self.rand = rand;
    }
}
