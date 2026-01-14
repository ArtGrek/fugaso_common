use crate::config::{bonanza_1000, thunder_express, BonanzaLinkCashConfig, ThunderExpressConfig};
use crate::protocol::{BonanzaLink1000Info, Stage, ThunderExpressInfo};
use crate::rand::{BonanzaLink1000Rand, BonanzaLink1000Random, ThunderExpressRand, ThunderExpressRandom};
use essential_core::err_on;
use essential_core::error::ServerError;
use fugaso_data::fugaso_action;
use fugaso_data::fugaso_action::ActionKind;
use fugaso_math::fsm::SlotFSM;
use fugaso_math::math::{BetCalculator, BetDenomCounterCalculator, GameInitArg, JoinArg, MathSettings, Request, SlotMath, SpinArg, Step};
use fugaso_math::protocol::{id, DatabaseStore, FreeGame, GameData, GameResult, InitialData, StartInfo};
use fugaso_math::protocol::{Gain, SpinData, Win};
use fugaso_math::validator::RequestValidator;
use log::{debug, info};
use std::sync::Arc;
use std::{usize, vec};

pub struct ThunderExpressMath<R: ThunderExpressRand> {
    pub result: Arc<GameData<ThunderExpressInfo, StartInfo>>,
    pub config: Arc<ThunderExpressConfig>,
    pub rand: R,
}

impl ThunderExpressMath<ThunderExpressRandom> {
    pub fn new(config: Option<String>, reels_cfg: Option<String>) -> Result<Self, ServerError> {
        let cfg = config.map(|j| serde_json::from_str(&j).map(|v| Arc::new(v)).map_err(|e| err_on!(e))).unwrap_or(Ok(Arc::clone(&thunder_express::CFG)))?;
        let reels_cfg_on = reels_cfg.map(|j| serde_json::from_str(&j).map(|v| Arc::new(v)).map_err(|e| err_on!(e))).unwrap_or(Ok(Arc::clone(&thunder_express::REELS_CFG)))?;
        let rand = ThunderExpressRandom::new(Arc::clone(&cfg), reels_cfg_on);
        Self::custom(rand, cfg)
    }
}

impl<R: ThunderExpressRand> ThunderExpressMath<R> {
    pub fn configured(rand: R) -> Result<Self, ServerError> {
        Self::custom(rand, Arc::clone(&thunder_express::CFG))
    }

    pub fn custom(mut rand: R, config: Arc<ThunderExpressConfig>) -> Result<Self, ServerError> {
        let category = thunder_express::BASE_CATEGORY;
        let (stops, grid) = rand.rand_cols_group(category, None)?;
        let mults = rand.rand_mults(&grid, 0, false)?;
        let special = if mults.len() > 0 {
            Some(ThunderExpressInfo {
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

    fn create_collects(&self, grid: &Vec<Vec<char>>, sum: i32, mults: &Vec<Vec<i32>>) -> Vec<Vec<i32>> {
        grid.iter()
            .enumerate()
            .map(|(c, col)| {
                col.iter()
                    .enumerate()
                    .map(|(r, v)| {
                        if *v == thunder_express::SYM_COLLECT {
                            sum + mults[c][r]
                        } else {
                            0
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    }

    pub fn check_lines(&mut self, req: &Request, counter_idx: usize, round_mul: i32, grid: &Vec<Vec<char>>) -> Result<(Vec<Gain>, Vec<i32>, ThunderExpressInfo), ServerError> {
        let lines = &self.config.lines;
        let combs = &self.config.wins;
        let mut gains = lines
            .iter()
            .enumerate()
            .filter_map(|(line_num, l)| {
                let mut w = grid[0][l[0]];
                let mut symbols = 0;

                for j in 0..l.len() {
                    let ch = grid[j][l[j]];
                    if w == thunder_express::SYM_WILD {
                        w = ch
                    }
                    if w == ch || ch == thunder_express::SYM_WILD {
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
            })
            .collect::<Vec<_>>();

        let scatters = grid.iter().flat_map(|c| c.iter().filter(|v| **v == thunder_express::SYM_COLLECT)).count();
        let coins = grid.iter().flat_map(|c| c.iter().filter(|v| thunder_express::is_coin(**v))).count();
        let overlay = if scatters + coins > 0 && scatters + coins < grid.len() || req.bet_counter > self.config.bet_counters[1] {
            self.rand.rand_over(grid, counter_idx)?
        } else {
            None
        };
        debug!("over: {overlay:?} counter_idx: {counter_idx}");
        let grid_on = overlay.as_ref().unwrap_or(grid);
        let scatters = grid_on.iter().flat_map(|c| c.iter().filter(|v| **v == thunder_express::SYM_COLLECT)).count();
        let coins = grid_on.iter().flat_map(|c| c.iter().filter(|v| thunder_express::is_coin(**v))).count();
        debug!("coins: {coins} scatters: {scatters}");

        let mults = self.rand.rand_mults(grid_on, counter_idx, false)?;
        let (mults1, mut respins) = if scatters > 0 && coins >= grid.len() - 1 {
            let sum = mults.iter().flat_map(|c| c.iter()).sum::<i32>();
            (self.create_collects(grid_on, sum, &mults), thunder_express::BONUS_COUNT)
        } else {
            let mults1 = if scatters > 0 && coins > 0 {
                let sum = mults.iter().flat_map(|c| c.iter()).sum::<i32>();
                let amount = sum as i64 * req.bet as i64 * req.denom as i64 * round_mul as i64;
                gains.extend((0..scatters).map(|_| Gain {
                    symbol: thunder_express::SYM_COLLECT,
                    amount,
                    multi: 1,
                    ..Default::default()
                }));
                self.create_collects(grid, sum, &mults)
            } else {
                vec![]
            };
            (mults1, 0)
        };
        debug!("mults1: {mults1:?}");
        let mut total = gains.iter().map(|w| w.amount).sum();

        let max = self.calc_max_win(req);
        let stop = if total >= max {
            respins = 0;
            total = max;
            Some(self.config.stop_factor)
        } else {
            None
        };

        let special = ThunderExpressInfo {
            mults,
            mults1,
            respins,
            overlay,
            total,
            stop,
            ..Default::default()
        };
        debug!("{special:?}");
        Ok((gains, vec![0], special))
    }

    pub fn check_bonus(
        &mut self,
        req: &Request,
        counter_idx: usize,
        multiplier: i32,
        mut grid: Vec<Vec<char>>,
        prev_grid: &Vec<Vec<char>>,
        prev_info: &ThunderExpressInfo,
        prev_total: i64,
    ) -> Result<(Vec<Vec<char>>, Vec<Gain>, ThunderExpressInfo, Vec<i32>), ServerError> {
        let prev_scatters = prev_grid.iter().flat_map(|c| c.iter().filter(|v| **v == thunder_express::SYM_COLLECT)).count();
        let scatters = grid.iter().flat_map(|c| c.iter().filter(|v| **v == thunder_express::SYM_COLLECT)).count();
        let coins = grid.iter().flat_map(|c| c.iter().filter(|v| thunder_express::is_coin(**v))).count();

        let mut mults = self.rand.rand_mults(&grid, counter_idx, true)?;
        debug!("mults: {mults:?}");
        for c in 0..mults.len() {
            for r in 0..mults[c].len() {
                if let Some(s) = self.config.map_jack.get(&mults[c][r]) {
                    grid[c][r] = *s;
                }
                if prev_info.mults1[c][r] > 0 {
                    mults[c][r] = prev_info.mults1[c][r];
                }
            }
        }
        debug!("coins: {coins:?}");

        let (mults1, mut respins) = if coins > 0 {
            let grid_on = &grid;
            let sum = mults
                .iter()
                .enumerate()
                .flat_map(|(c, col)| {
                    col.iter().enumerate().map(move |(r, v)| {
                        if thunder_express::is_coin(grid_on[c][r]) {
                            *v
                        } else {
                            0
                        }
                    })
                })
                .sum::<i32>();
            debug!("sum:{sum}");
            let mults1 = self.create_collects(&grid, sum, &prev_info.mults1);
            (mults1, thunder_express::BONUS_COUNT)
        } else {
            let respins = if scatters > prev_scatters {
                thunder_express::BONUS_COUNT
            } else {
                prev_info.respins - 1
            };
            (prev_info.mults1.clone(), respins)
        };

        let max = self.calc_max_win(req);
        let gains_end = self.calc_gains(req, multiplier, &mults1);
        let stop = if prev_total + gains_end.iter().map(|g| g.amount).sum::<i64>() >= max {
            respins = 0;
            Some(self.config.stop_factor)
        } else {
            None
        };

        let gains = if respins == 0 {
            gains_end
        } else {
            vec![]
        };
        let sum = gains.iter().map(|g| g.amount).sum::<i64>();
        let total = std::cmp::min(max, prev_total + sum);
        let accum = std::cmp::min(max, prev_info.accum + sum);

        Ok((
            grid,
            gains,
            ThunderExpressInfo {
                mults,
                mults1,
                respins,
                total,
                accum,
                stop,
                ..Default::default()
            },
            vec![0],
        ))
    }

    fn calc_gains(&self, req: &Request, round_mul: i32, mults1: &Vec<Vec<i32>>) -> Vec<Gain> {
        mults1
            .iter()
            .flat_map(|c| c.iter())
            .filter(|h| **h > 0)
            .map(|h| {
                let amount = *h as i64 * req.bet as i64 * req.denom as i64 * round_mul as i64;
                Gain {
                    symbol: thunder_express::SYM_COLLECT,
                    count: 1,
                    amount,
                    line_num: 0,
                    multi: 1,
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>()
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
                if prev[c][r] == thunder_express::SYM_COLLECT {
                    current[c][r] = thunder_express::SYM_COLLECT
                }
            }
        }
    }
}

impl<R: ThunderExpressRand> SlotMath for ThunderExpressMath<R> {
    type Special = ThunderExpressInfo;
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

        Ok(GameData::Initial(InitialData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            min_bet: 0,
            max_bet: 0,
            lines: self.config.lines.clone(),
            reels: self.config.reels.clone(),
            wins: self
                .config
                .wins
                .iter()
                .flat_map(|p| {
                    p.1.iter().map(|w| Win {
                        symbol: *p.0,
                        count: *w.0,
                        factor: *w.1,
                    })
                })
                .collect(),
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

    fn init(&mut self, arg: GameInitArg, actions: &Vec<fugaso_action::Model>) -> Result<(), ServerError> {
        if actions.is_empty() {
            return Ok(());
        }
        let action = &actions[actions.len() - 1];
        if let Some(next) = &action.next_act {
            let result0: GameResult<Self::Special, Self::Restore> = GameResult::from_action(&actions[0])?;
            let mut result_on = GameResult::from_action(action)?;

            let restore = match result0.special.as_ref() {
                Some(ThunderExpressInfo {
                    mults,
                    overlay: Some(o),
                    ..
                }) => Some(StartInfo {
                    mults: mults.clone(),
                    grid: Some(o.clone()),
                    ..Default::default()
                }),
                Some(ThunderExpressInfo {
                    mults,
                    ..
                }) => Some(StartInfo {
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
                result_on.total = result_on.special.as_ref().map(|s| s.total).unwrap_or(result_on.total);
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

    fn spin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let count_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet counter!"))?;
        let (category, stops, grid) = if request.bet_counter < self.config.bet_counters[2] {
            let category = thunder_express::BASE_CATEGORY + count_idx;
            let (stops, grid) = self.rand.rand_cols_group(category, combo)?;
            (category, stops, grid)
        } else if request.bet_counter == self.config.bet_counters[2] {
            let category = thunder_express::BASE_CATEGORY;
            let (stops, grid) = self.rand.rand_cols_group(category, None)?;
            (category, stops, grid)
        } else if request.bet_counter == self.config.bet_counters[3] {
            let category = thunder_express::BASE_CATEGORY;
            let (stops, grid) = self.rand.rand_buy_cols(category)?;
            (category, stops, grid)
        } else {
            return Err(err_on!("illegal bet_counter!"));
        };

        let (gains, holds, special) = self.check_lines(request, count_idx, arg.round_multiplier, &grid)?;
        let total = gains.iter().map(|g| g.amount).sum();
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

    fn respin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let prev = Arc::clone(&self.result);
        let (prev_total, prev_info, prev_grid, prev_restore) = match prev.as_ref() {
            GameData::Spin(v) => {
                let prev_info = v.result.special.as_ref().ok_or_else(|| err_on!("Illegal state!"))?;
                let grid = if let Some(special) = &v.result.special {
                    if let Some(over) = special.overlay.as_ref() {
                        over
                    } else {
                        &v.result.grid
                    }
                } else {
                    &v.result.grid
                };
                (v.result.total, prev_info, grid, &v.result.restore)
            }
            GameData::ReSpin(v) => {
                let prev_info = v.result.special.as_ref().ok_or_else(|| err_on!("Illegal state!"))?;
                (v.result.total, prev_info, &v.result.grid, &v.result.restore)
            }
            _ => return Err(err_on!("Illegal state!")),
        };
        let counter_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet_counter!"))?;
        let category = thunder_express::BONUS_OFFSET + counter_idx;
        let (stops, mut grid) = self.rand.rand_cols(category, combo);
        self.apply_prev(&mut grid, prev_grid);
        debug!("{grid:?}");

        let (grid, gains, special, holds) = self.check_bonus(request, counter_idx, arg.round_multiplier, grid, prev_grid, prev_info, prev_total)?;
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

    fn post_process(&mut self, kind: ActionKind, mut data: GameData<Self::Special, Self::Restore>) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError> {
        data.set_next_act(kind);
        self.result = Arc::new(data);
        Ok(self.result.clone())
    }

    fn close(&self, next_act: ActionKind) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
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

    fn collect(&self, request: &Request, arg: SpinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let (category, result, free_game) = match self.result.as_ref() {
            GameData::Initial(v) => (v.category, v.result.clone(), v.free.clone()),
            GameData::Spin(v) | GameData::ReSpin(v) => (v.category, v.result.clone(), v.free.clone()),
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

pub struct BonanzaLink1000Math<R: BonanzaLink1000Rand> {
    pub result: Arc<GameData<BonanzaLink1000Info, StartInfo>>,
    pub config: Arc<BonanzaLinkCashConfig>,
    pub rand: R,
}

impl BonanzaLink1000Math<BonanzaLink1000Random> {
    pub fn new(config: Option<String>) -> Result<Self, ServerError> {
        let cfg = config.map(|j| serde_json::from_str(&j).map(|v| Arc::new(v)).map_err(|e| err_on!(e))).unwrap_or(Ok(Arc::clone(&bonanza_1000::CFG)))?;
        let rand = BonanzaLink1000Random::new(Arc::clone(&cfg));
        Self::custom(rand, cfg)
    }
}

impl<R: BonanzaLink1000Rand> BonanzaLink1000Math<R> {
    pub fn configured(rand: R) -> Result<Self, ServerError> {
        Self::custom(rand, Arc::clone(&bonanza_1000::CFG))
    }

    pub fn custom(mut rand: R, config: Arc<BonanzaLinkCashConfig>) -> Result<Self, ServerError> {
        let category = bonanza_1000::BASE_CATEGORY;
        let (stops, grid) = rand.rand_cols(category, None);
        let holds = vec![0];
        let special = Some(BonanzaLink1000Info {
            mults: rand.rand_mults(&grid)?,
            ..Default::default()
        });
        let m = Self {
            rand,
            result: Arc::new(GameData::Spin(SpinData {
                id: id::GAME_DATA,
                result: GameResult {
                    stops,
                    holds,
                    grid,
                    special,
                    ..Default::default()
                },
                category,
                ..Default::default()
            })),
            config,
        };
        Ok(m)
    }

    pub fn check_lines(
        &mut self,
        req: &Request,
        multiplier: i32,
        category: usize,
        grid: &Vec<Vec<char>>,
        prev_total: i64,
        respin: bool,
    ) -> Result<(Vec<Gain>, BonanzaLink1000Info, i32), ServerError> {
        let lines = &self.config.lines;
        let combs = &self.config.wins;

        let scatters = grid
            .iter()
            .flat_map(|c| {
                c.iter().enumerate().filter_map(|(r, v)| {
                    if *v == bonanza_1000::SYM_SCAT {
                        Some(r)
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();

        let (respins, over) = if scatters.len() == 2 {
            let respins = if scatters.iter().all(|r| *r < grid[0].len() - 1) {
                1
            } else {
                0
            };
            let over = if !respin && respins == 0 {
                self.rand.rand_pull(category, grid, vec![bonanza_1000::SYM_SCAT], bonanza_1000::SYM_SCAT)?
            } else {
                None
            };
            (respins, over)
        } else {
            (0, None)
        };

        let grid_on = over.as_ref().map(|o| &o.grid).unwrap_or(grid);
        let scatters_len = grid_on.iter().flat_map(|c| c.iter().filter(|v| **v == bonanza_1000::SYM_SCAT)).count();

        let (free_games, stage_max) = if scatters_len > 2 {
            let stage_max = Some(bonanza_1000::STEPS_ON_LEVEL);
            ((scatters_len - 1) as i32 * bonanza_1000::FREE_GAME_FACTOR, stage_max)
        } else {
            (0, None)
        };

        let wins = lines
            .iter()
            .enumerate()
            .filter_map(|(line_num, l)| {
                let mut w = grid_on[0][l[0]];
                let mut symbols = 0;

                for j in 0..l.len() {
                    let ch = grid_on[j][l[j]];
                    if ch == bonanza_1000::SYM_SCAT {
                        break;
                    }

                    if bonanza_1000::SYM_WILD == w {
                        w = ch
                    }

                    if w == ch || bonanza_1000::SYM_WILD == ch {
                        symbols += 1;
                    } else {
                        break;
                    }
                }
                let factor = *combs.get(&w).and_then(|m| m.get(&symbols)).unwrap_or(&0);

                if factor > 0 {
                    let amount = factor as i64 * req.bet as i64 * multiplier as i64;
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
            })
            .collect::<Vec<_>>();

        let max = self.calc_max_win(req);
        let sum_current = wins.iter().map(|w| w.amount).sum::<i64>();

        let (stop, free_games, respins) = if sum_current + prev_total >= max {
            (Some(self.config.stop_factor), 0, 0)
        } else {
            (None, free_games, respins)
        };
        let total = std::cmp::min(max, prev_total + sum_current);

        let mults = self.rand.rand_mults(grid)?;
        let special = BonanzaLink1000Info {
            respins,
            total,
            mults,
            over,
            stage_max,
            stop,
            ..Default::default()
        };

        Ok((wins, special, free_games))
    }

    pub fn check_free(
        &mut self,
        req: &Request,
        multiplier: i32,
        category: usize,
        grid: &Vec<Vec<char>>,
        prev_info: &BonanzaLink1000Info,
    ) -> Result<(Vec<Gain>, BonanzaLink1000Info, i32), ServerError> {
        let wild0 = self.count_wilds(grid);
        let coins = grid.iter().flat_map(|c| c.iter().filter(|v| **v == bonanza_1000::SYM_COIN)).count();
        let over = if wild0 > 0 && coins == 0 {
            self.rand.rand_over_coins(category, &grid)?
        } else if wild0 == 0 && coins > 0 {
            self.rand.rand_pull(bonanza_1000::FREE_CATEGORY, grid, vec![bonanza_1000::SYM_WILD], bonanza_1000::SYM_COIN)?
        } else {
            None
        };
        debug!("coins: {coins:?} over: {over:?}");
        let grid_on = over.as_ref().map(|o| &o.grid).unwrap_or(grid);
        let wild0 = self.count_wilds(grid_on);

        let prev_stages = if prev_info.stages.is_empty() {
            vec![Stage::default()]
        } else {
            prev_info.stages.clone()
        };
        let stage_max = prev_info.stage_max.ok_or_else(|| err_on!("stage_max is none!"))?;
        let level = vec![std::cmp::min(bonanza_1000::LEVEL_MAX - 1, (prev_stages[0].total + wild0) / stage_max)];
        let stages = vec![Stage {
            total: prev_stages[0].total + wild0,
            level: level[0],
            current: wild0,
            shift: if level[0] > prev_stages[0].level {
                1
            } else {
                0
            },
        }];

        let lines = &self.config.lines;
        let combs = &self.config.wins;
        let mut wins = lines
            .iter()
            .enumerate()
            .filter_map(|(line_num, l)| {
                let mut w = grid_on[0][l[0]];
                let mut symbols = 0;

                for j in 0..l.len() {
                    let ch = grid_on[j][l[j]];
                    if ch == bonanza_1000::SYM_SCAT {
                        break;
                    }

                    if bonanza_1000::SYM_WILD == w {
                        w = ch
                    }

                    if w == ch || bonanza_1000::SYM_WILD == ch {
                        symbols += 1;
                    } else {
                        break;
                    }
                }
                let factor = *combs.get(&w).and_then(|m| m.get(&symbols)).unwrap_or(&0);

                if factor > 0 {
                    let amount = factor as i64 * req.bet as i64 * multiplier as i64;
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
            })
            .collect::<Vec<_>>();

        let mults = self.rand.rand_mults(grid_on)?;
        let mult_sum = mults.iter().flat_map(|m| m).sum::<i32>();

        if wild0 > 0 {
            let multi = bonanza_1000::MULT_LEVELS[prev_stages[0].level];
            wins.push(Gain {
                symbol: bonanza_1000::SYM_WILD,
                count: wild0,
                amount: mult_sum as i64 * req.bet as i64 * req.denom as i64 * wild0 as i64 * multi as i64,
                multi,
                ..Default::default()
            });
        }

        debug!("gains: {wins:?}");
        let free_wild0 = if stages[0].shift > 0 {
            bonanza_1000::FREE_GAMES_LEVEL
        } else {
            0
        };

        let max = self.calc_max_win(req);
        let sum_current = wins.iter().map(|w| w.amount).sum::<i64>();

        let (stop, free_games) = if sum_current + prev_info.total >= max {
            (Some(self.config.stop_factor), 0)
        } else {
            (None, free_wild0)
        };
        let total = std::cmp::min(max, prev_info.total + sum_current);

        let special = BonanzaLink1000Info {
            total,
            over,
            mults,
            stages,
            stage_max: prev_info.stage_max,
            stop,
            ..Default::default()
        };

        Ok((wins, special, free_games))
    }

    fn count_wilds(&self, grid: &Vec<Vec<char>>) -> usize {
        grid.iter().flat_map(|c| c.iter().filter(|v| bonanza_1000::SYM_WILD == **v)).count()
    }

    fn calc_max_win(&self, req: &Request) -> i64 {
        let calculator = self.create_bet_calculator();
        let playing_bet = calculator.calc_playing_bet(&req);
        let max = playing_bet * self.config.stop_factor as i64;
        max
    }

    fn calc_free_category(&self, count_idx: usize) -> usize {
        bonanza_1000::FREE_CATEGORY + count_idx * bonanza_1000::LEVEL_MAX
    }
}

impl<R: BonanzaLink1000Rand> SlotMath for BonanzaLink1000Math<R> {
    type Special = BonanzaLink1000Info;
    type Calculator = BetDenomCounterCalculator;
    type Restore = StartInfo;
    type PlayFSM = SlotFSM;
    type Rand = R;
    type Input = Request;
    type V = RequestValidator;

    fn join(&self, arg: JoinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let (result, free) = match self.result.as_ref() {
            GameData::Initial(v) => (v.result.clone(), v.free.clone()),
            GameData::Spin(v) | GameData::ReSpin(v) | GameData::FreeSpin(v) => (v.result.clone(), v.free.clone()),
            GameData::Collect(v) => (v.result.clone(), v.free.clone()),
        };

        Ok(GameData::Initial(InitialData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            min_bet: 0,
            max_bet: 0,
            lines: self.config.lines.clone(),
            reels: self.config.reels.clone(),
            wins: self
                .config
                .wins
                .iter()
                .flat_map(|p| {
                    p.1.iter().map(|w| Win {
                        symbol: *p.0,
                        count: *w.0,
                        factor: *w.1,
                    })
                })
                .collect(),
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
            free,
            ..Default::default()
        }))
    }

    fn init(&mut self, arg: GameInitArg, actions: &Vec<fugaso_action::Model>) -> Result<(), ServerError> {
        if actions.is_empty() {
            return Ok(());
        }
        let action = &actions[actions.len() - 1];
        if let Some(next) = &action.next_act {
            let result0: GameResult<Self::Special, Self::Restore> = if let Some(a) = actions.iter().rev().find(|a| a.act_descr == Some(ActionKind::RESPIN)) {
                GameResult::from_action(&a)?
            } else {
                GameResult::from_action(&actions[0])?
            };
            let mut result_on: GameResult<Self::Special, Self::Restore> = GameResult::from_action(action)?;

            let restore = match result0.special.as_ref() {
                Some(BonanzaLink1000Info {
                    mults,
                    ..
                }) => Some(StartInfo {
                    mults: mults.clone(),
                    grid: Some(result0.grid.clone()),
                    ..Default::default()
                }),
                None => None,
            };

            let free = if let Some(f) = action.free_games.as_ref() {
                Some(FreeGame::from_db(f)?)
            } else {
                None
            };

            if action.act_descr == Some(ActionKind::BET) || action.act_descr == Some(ActionKind::RESPIN) {
                result_on.total = result_on.special.as_ref().map(|s| s.total).unwrap_or(result_on.total);
            } else if action.act_descr == Some(ActionKind::FREE_SPIN) {
                result_on.total = free.as_ref().map(|f| f.total_win).unwrap_or(result_on.total);
            }

            if *next == ActionKind::BET {
                result_on.stops = result0.stops;
                result_on.grid = result0.grid;
                result_on.special = result0.special;
                result_on.holds = result0.holds;
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
                free,
            };
            match action.act_descr.as_ref() {
                None => {}
                Some(a) => match a {
                    ActionKind::RESPIN => self.result = Arc::new(GameData::ReSpin(spin_data)),
                    ActionKind::FREE_SPIN => self.result = Arc::new(GameData::FreeSpin(spin_data)),
                    _ => self.result = Arc::new(GameData::Spin(spin_data)),
                },
            }
        }
        Ok(())
    }

    fn spin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let count_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet counter!"))?;
        let (category, stops, grid) = if count_idx == 1 {
            let category = bonanza_1000::X5_CATEGORY;
            let (stops, grid) = self.rand.rand_cols(category, combo);
            (category, stops, grid)
        } else if count_idx > 1 {
            let category = bonanza_1000::BASE_CATEGORY;
            let (stops, grid) = self.rand.rand_buy_cols(category)?;
            (category, stops, grid)
        } else {
            let category = bonanza_1000::BASE_CATEGORY;
            let (stops, grid) = self.rand.rand_cols(category, combo);
            (category, stops, grid)
        };

        let (gains, special, free_games) = self.check_lines(request, arg.round_multiplier, category, &grid, 0, false)?;
        let (next_act, restore, free_game) = if special.respins > 0 {
            (
                ActionKind::RESPIN,
                Some(StartInfo {
                    grid: Some(grid.clone()),
                    mults: special.mults.clone(),
                    ..Default::default()
                }),
                FreeGame::default(),
            )
        } else if free_games > 0 {
            (
                ActionKind::FREE_SPIN,
                None,
                FreeGame {
                    total_win: special.total,
                    symbol: '?',
                    category: self.calc_free_category(count_idx),
                    initial: free_games,
                    left: free_games,
                    done: 0,
                },
            )
        } else {
            (ActionKind::CLOSE, None, FreeGame::default())
        };
        let result = GameData::Spin(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance - arg.stake,
            credit_type: 100,
            result: GameResult {
                total: special.total,
                stops,
                holds: vec![0],
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
            free: Some(free_game),
            ..Default::default()
        });
        Ok(result)
    }

    fn free_spin(&mut self, request: &Request, arg: SpinArg, _step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let prev = Arc::clone(&self.result);
        let (_prev_grid, prev_info) = match prev.as_ref() {
            GameData::Spin(v) | GameData::FreeSpin(v) | GameData::ReSpin(v) => {
                let spec = v.result.special.as_ref().ok_or_else(|| err_on!("special is none!"))?;
                (&v.result.grid, spec)
            }
            _ => return Err(err_on!("Illegal state!")),
        };
        let prev_free = prev.free().ok_or_else(|| err_on!("wrong state - free is none!"))?;

        let count_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet counter!"))?;
        let category_start = self.calc_free_category(count_idx);
        let category = category_start + prev_info.stages.iter().map(|s| s.level).sum::<usize>();
        let (stops, grid) = self.rand.rand_cols(category, combo);
        let (gains, special, free_games) = self.check_free(request, arg.round_multiplier, category, &grid, prev_info)?;
        let (next_act, restore) = if special.respins > 0 {
            (
                ActionKind::RESPIN,
                Some(StartInfo {
                    grid: Some(grid.clone()),
                    mults: special.mults.clone(),
                    ..Default::default()
                }),
            )
        } else {
            (ActionKind::CLOSE, None)
        };

        let mut free_game = prev_free.clone();
        free_game.play();
        free_game.category = category;
        free_game.total_win = special.total;
        if special.stop.is_some() {
            free_game.left = 0
        } else {
            free_game.add(free_games);
        }

        let result = GameData::FreeSpin(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            result: GameResult {
                total: special.total,
                stops,
                grid,
                gains,
                holds: vec![0],
                special: Some(special),
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
            free: Some(free_game),
        });
        Ok(result)
    }

    fn respin(&mut self, request: &Request, arg: SpinArg, _step: &Step, _combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let prev = Arc::clone(&self.result);
        let (prev_total, prev_grid, prev_category, prev_stops) = match prev.as_ref() {
            GameData::Spin(v) | GameData::FreeSpin(v) => (v.result.total, &v.result.grid, v.category, &v.result.stops),
            GameData::ReSpin(v) => (v.result.total, &v.result.grid, v.category, &v.result.stops),
            _ => return Err(err_on!("Illegal state!")),
        };
        let count_idx = self.config.bet_counters.iter().position(|c| *c == request.bet_counter).ok_or_else(|| err_on!("illegal bet counter!"))?;
        let category = prev_category;
        let (stops, grid) = self.rand.rand_respin_cols(category, prev_grid, prev_stops);

        let (gains, special, free_games) = self.check_lines(&request, arg.round_multiplier, category, &grid, prev_total, true)?;
        let (next_act, restore, free_game) = if special.respins > 0 {
            (
                ActionKind::RESPIN,
                Some(StartInfo {
                    grid: Some(grid.clone()),
                    mults: special.mults.clone(),
                    ..Default::default()
                }),
                FreeGame::default(),
            )
        } else if free_games > 0 {
            (
                ActionKind::FREE_SPIN,
                None,
                FreeGame {
                    total_win: special.total,
                    symbol: '?',
                    category: self.calc_free_category(count_idx),
                    initial: free_games,
                    left: free_games,
                    done: 0,
                },
            )
        } else {
            (ActionKind::CLOSE, None, FreeGame::default())
        };

        let result = GameData::ReSpin(SpinData {
            id: id::GAME_DATA,
            balance: arg.balance,
            credit_type: 100,
            result: GameResult {
                total: special.total,
                stops,
                holds: vec![0],
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
            free: Some(free_game),
            ..Default::default()
        });
        Ok(result)
    }

    fn post_process(&mut self, kind: ActionKind, mut data: GameData<Self::Special, Self::Restore>) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError> {
        data.set_next_act(kind);
        self.result = Arc::new(data);
        Ok(self.result.clone())
    }

    fn close(&self, next_act: ActionKind) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        match self.result.as_ref() {
            GameData::Spin(v) => Ok(GameData::Spin(SpinData {
                next_act,
                ..v.clone()
            })),
            GameData::ReSpin(v) => Ok(GameData::ReSpin(SpinData {
                next_act,
                ..v.clone()
            })),
            GameData::FreeSpin(v) => Ok(GameData::FreeSpin(SpinData {
                next_act,
                ..v.clone()
            })),
            _ => Err(err_on!("Illegal state!")),
        }
    }

    fn collect(&self, request: &Request, arg: SpinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let (category, result, free_game) = match self.result.as_ref() {
            GameData::Initial(v) => (v.category, v.result.clone(), v.free.clone()),
            GameData::Spin(v) | GameData::ReSpin(v) | GameData::FreeSpin(v) | GameData::Collect(v) => (v.category, v.result.clone(), v.free.clone()),
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
