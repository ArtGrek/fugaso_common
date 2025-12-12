use crate::fsm::FSM;
use crate::protocol::{id, DatabaseStore, FreeGame, GameResult, SpinData};
use crate::protocol::{GameData, Promo};
use crate::validator::{SimpleValidator, Validator};
use essential_core::err_on;
use essential_core::error::ServerError;
use essential_rand::random::RandomGenerator;
use fugaso_data::fugaso_action::ActionKind;
use fugaso_data::fugaso_round::RoundDetail;
use fugaso_data::{fugaso_action, fugaso_round};
use log::{debug, warn};
use num_traits::cast::ToPrimitive;
use sea_orm::prelude::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;

pub struct MathSettings {
    pub lines: Vec<usize>,
    pub reels: Vec<usize>,
    pub bet_counters: Vec<usize>,
}

pub trait IPlayResponse {
    fn create_action_default(&self) -> Result<fugaso_action::Model, ServerError>;

    fn free(&self) -> Option<&FreeGame>;

    fn has_bonus(&self) -> bool;

    fn has_respin(&self) -> bool;

    fn has_drop(&self) -> bool;

    fn total(&self) -> i64;

    fn respins(&self) -> i32 {
        0
    }

    fn is_gamble_end(&self, total_bet: i64) -> bool;

    fn stops_on(&self) -> Vec<usize>;

    fn grid_on(&self) -> Vec<Vec<char>>;

    fn promo(&self) -> Promo;

    fn set_next_act(&mut self, kind: ActionKind);
}

#[derive(Debug)]
pub struct JoinArg {
    pub balance: i64,
    pub round_id: i64,
    pub round_type: RoundDetail,
    pub round_multiplier: i32,
    pub curr_lines: usize,
    pub curr_bet: i32,
    pub curr_denom: i32,
    pub curr_reels: usize,
    pub bet_counter: usize,
    pub next_act: ActionKind,
    pub poss_lines: Vec<usize>,
    pub poss_reels: Vec<usize>,
    pub poss_bets: Vec<i32>,
    pub poss_denom: Vec<i32>,
    pub poss_bet_counters: Vec<usize>,
    pub promo: Promo,
}

#[derive(Debug)]
pub struct GameInitArg {
    pub curr_lines: usize,
    pub curr_bet: i32,
    pub curr_denom: i32,
    pub curr_reels: usize,
    pub round_id: i64,
    pub round_type: RoundDetail,
    pub round_multiplier: i32,
    pub bet_counter: usize,
    pub promo: Promo,
}

#[derive(Debug, Clone)]
pub struct SpinArg {
    pub balance: i64,
    pub round_id: i64,
    pub round_type: RoundDetail,
    pub round_multiplier: i32,
    pub next_act: ActionKind,
    pub promo: Promo,
    pub stake: i64,
}

pub trait SlotMath {
    type Special: DatabaseStore + Default + 'static;
    type Restore: Default + 'static;
    type PlayFSM: FSM;
    type Rand;
    type Input: DeserializeOwned + IRequest + Default;
    type Calculator: BetCalculator<I = Self::Input> + Default;
    type V: Validator<I = Self::Input> + SimpleValidator + Default;

    fn join(&self, arg: JoinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError>;

    fn init(&mut self, arg: GameInitArg, actions: &Vec<fugaso_action::Model>) -> Result<(), ServerError>;

    fn spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError>;

    #[allow(unused_variables)]
    fn free_spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        Err(err_on!("free_spin is not supported!"))
    }

    #[allow(unused_variables)]
    fn respin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        Err(err_on!("respin is not supported!"))
    }

    fn post_process(&mut self, kind: ActionKind, game_data: GameData<Self::Special, Self::Restore>) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError>;

    fn close(&self, next_act: ActionKind) -> Result<GameData<Self::Special, Self::Restore>, ServerError>;

    fn collect(&self, request: &Self::Input, arg: SpinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError>;

    fn create_bet_calculator(&self) -> Self::Calculator {
        Self::Calculator::default()
    }

    fn create_fsm(&self, game_name: &str) -> Self::PlayFSM {
        Self::PlayFSM::default(game_name)
    }

    fn settings(&self) -> MathSettings;

    #[allow(unused_variables)]
    fn set_rand(&mut self, rand: Self::Rand);
}

pub trait SlotBaseMath {
    type M: SlotMath;
    type Calculator: BetCalculator<I = <Self::M as SlotMath>::Input> + Default;

    fn parent(&self) -> &Self::M;

    fn parent_mut(&mut self) -> &mut Self::M;

    fn join(&self, arg: JoinArg) -> Result<GameData<<Self::M as SlotMath>::Special, <Self::M as SlotMath>::Restore>, ServerError> {
        let p = self.parent();
        p.join(arg)
    }

    fn spin(
        &mut self,
        request: &<Self::M as SlotMath>::Input,
        arg: SpinArg,
        step: &Step,
        combo: Option<Vec<usize>>,
    ) -> Result<GameData<<Self::M as SlotMath>::Special, <Self::M as SlotMath>::Restore>, ServerError> {
        let p = self.parent_mut();
        p.spin(request, arg, step, combo)
    }

    fn respin(&mut self, request: &<Self::M as SlotMath>::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<<Self::M as SlotMath>::Special, <Self::M as SlotMath>::Restore>, ServerError> {
        let p = self.parent_mut();
        p.respin(request, arg, step, combo)
    }
}

impl<S: SlotBaseMath> SlotMath for S {
    type Special = <S::M as SlotMath>::Special;

    type Restore = <S::M as SlotMath>::Restore;

    type PlayFSM = <S::M as SlotMath>::PlayFSM;

    type Rand = <S::M as SlotMath>::Rand;

    type Input = <S::M as SlotMath>::Input;

    type Calculator = S::Calculator;

    type V = <S::M as SlotMath>::V;

    fn join(&self, arg: JoinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.join(arg)
    }

    fn init(&mut self, arg: GameInitArg, actions: &Vec<fugaso_action::Model>) -> Result<(), ServerError> {
        let p = self.parent_mut();
        p.init(arg, actions)
    }

    fn spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.spin(request, arg, step, combo)
    }

    fn free_spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let p = self.parent_mut();
        p.free_spin(request, arg, step, combo)
    }

    fn respin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.respin(request, arg, step, combo)
    }

    fn post_process(&mut self, kind: ActionKind, game_data: GameData<Self::Special, Self::Restore>) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError> {
        let p = self.parent_mut();
        p.post_process(kind, game_data)
    }

    fn close(&self, next_act: ActionKind) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let p = self.parent();
        p.close(next_act)
    }

    fn collect(&self, request: &Self::Input, arg: SpinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let p = self.parent();
        p.collect(request, arg)
    }

    fn settings(&self) -> MathSettings {
        let p = self.parent();
        p.settings()
    }

    fn set_rand(&mut self, rand: Self::Rand) {
        let p = self.parent_mut();
        p.set_rand(rand)
    }
}

pub trait IRequest {
    fn bet(&self) -> i32;
    fn line(&self) -> usize;
    fn denom(&self) -> i32;
    fn bet_index(&self) -> usize;
    fn bet_counter(&self) -> usize;
    fn reels(&self) -> usize;
    fn create_input(bet: i32, line: usize, denom: i32, bet_counter: usize) -> Self;
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub bet: i32,
    pub line: usize,
    pub denom: i32,
    #[serde(default)]
    pub bet_index: usize,
    #[serde(default)]
    pub bet_counter: usize,
    #[serde(default)]
    pub reels: usize,
}

impl IRequest for Request {
    fn bet(&self) -> i32 {
        self.bet
    }

    fn line(&self) -> usize {
        self.line
    }

    fn denom(&self) -> i32 {
        self.denom
    }

    fn bet_index(&self) -> usize {
        self.bet_index
    }

    fn bet_counter(&self) -> usize {
        self.bet_counter
    }

    fn reels(&self) -> usize {
        self.reels
    }

    fn create_input(bet: i32, line: usize, denom: i32, bet_counter: usize) -> Self {
        Request {
            bet,
            line,
            denom,
            bet_index: 0,
            bet_counter,
            reels: 1,
        }
    }
}

#[derive(Debug)]
pub struct GamePlayInput {
    pub line: usize,
    pub bet: Vec<i32>,
    pub denomination: Vec<i32>,
    pub side_bet: usize,
}

#[derive(Debug)]
pub struct GameInput {
    pub bet: i32,
    pub denom: i32,
    pub line: usize,
    pub stake: i64,
}

pub trait BetCalculator {
    type I: IRequest;
    fn calc_total_bet(&self, request: &Self::I) -> i64 {
        request.bet() as i64 * request.denom() as i64 * request.line() as i64
    }

    fn calc_playing_bet(&self, request: &Self::I) -> i64 {
        self.calc_total_bet(request)
    }

    fn calc_inputs(&self, game_play: GamePlayInput) -> Vec<GameInput> {
        let end = if game_play.denomination.len() > 2 {
            game_play.denomination.len()
        } else {
            1
        };
        let denom = &game_play.denomination;
        game_play
            .bet
            .iter()
            .flat_map(|b| {
                denom.iter().take(end).map(|d| {
                    let r = Self::I::create_input(*b, game_play.line, *d, game_play.side_bet);
                    let stake = self.calc_total_bet(&r);
                    GameInput {
                        bet: *b,
                        denom: *d,
                        line: game_play.line,
                        stake,
                    }
                })
            })
            .collect::<Vec<_>>()
        /* return Arrays.stream(bet)
        .boxed()
        .flatMap(b -> Arrays.stream(denomination, 0, end)
            .boxed()
            .map(d -> {
                long stake = calcTotalBet(line, b, d, sideBet);
                return new GameInput(b, d, line, stake);
            }
            ))
        .sorted(Comparator.comparingLong(g -> g.getStake()))
        .collect(Collectors.toList());*/
    }
}

#[derive(Default)]
pub struct DefaultBetCalculator;

impl BetCalculator for DefaultBetCalculator {
    type I = Request;
    fn calc_total_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64 * request.line as i64
    }

    fn calc_playing_bet(&self, request: &Request) -> i64 {
        self.calc_total_bet(request)
    }

    fn calc_inputs(&self, game_play: GamePlayInput) -> Vec<GameInput> {
        let end = if game_play.denomination.len() > 2 {
            game_play.denomination.len()
        } else {
            1
        };
        let denom = &game_play.denomination;
        game_play
            .bet
            .iter()
            .flat_map(|b| {
                denom.iter().take(end).map(|d| {
                    let stake = self.calc_total_bet(&Request {
                        bet: *b,
                        line: game_play.line,
                        denom: *d,
                        bet_index: 0,
                        bet_counter: game_play.side_bet,
                        reels: 1,
                    });
                    GameInput {
                        bet: *b,
                        denom: *d,
                        line: game_play.line,
                        stake,
                    }
                })
            })
            .collect::<Vec<_>>()
        /* return Arrays.stream(bet)
        .boxed()
        .flatMap(b -> Arrays.stream(denomination, 0, end)
            .boxed()
            .map(d -> {
                long stake = calcTotalBet(line, b, d, sideBet);
                return new GameInput(b, d, line, stake);
            }
            ))
        .sorted(Comparator.comparingLong(g -> g.getStake()))
        .collect(Collectors.toList());*/
    }
}

#[derive(Default)]
pub struct BetDenomCalculator;

impl BetCalculator for BetDenomCalculator {
    type I = Request;
    fn calc_total_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64
    }
}

#[derive(Default)]
pub struct BetDenomCounterCalculator;

impl BetCalculator for BetDenomCounterCalculator {
    type I = Request;
    fn calc_playing_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64
    }
    fn calc_total_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64 * request.bet_counter as i64
    }
}

#[derive(Default)]
pub struct BetLineCounterCalculator;

impl BetCalculator for BetLineCounterCalculator {
    type I = Request;
    fn calc_playing_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64 * request.line as i64
    }
    fn calc_total_bet(&self, request: &Request) -> i64 {
        request.bet as i64 * request.denom as i64 * request.line as i64 * request.bet_counter as i64
    }
}

#[derive(Default)]
pub struct PowerCoinTrintyCalculator;

impl BetCalculator for PowerCoinTrintyCalculator {
    type I = Request;
    fn calc_playing_bet(&self, request: &Request) -> i64 {
        if request.bet_counter > 1 {
            request.bet as i64 * request.denom as i64
        } else {
            request.bet as i64 * request.denom as i64 * request.line as i64
        }
    }
    fn calc_total_bet(&self, request: &Request) -> i64 {
        if request.bet_counter > 1 {
            request.bet as i64 * request.denom as i64 * request.bet_counter as i64
        } else {
            request.bet as i64 * request.denom as i64 * request.line as i64
        }
    }
}

pub const MIN_TAKE: usize = 80;
pub const MAX_TAKE: usize = 100;
pub const MIN_ALLOWED_EUR: i64 = 25;
#[derive(Debug, Clone)]
pub struct Step {
    pub win: i64,
    pub take: usize,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            win: i64::MAX,
            take: MAX_TAKE,
        }
    }
}

const MAX_ATTEMPTS: usize = 100;
pub struct ProxyMath<M: SlotMath> {
    imp: M,
    rand: RandomGenerator,
}

impl<M: SlotMath> ProxyMath<M> {
    pub fn new(m: M) -> Self {
        Self {
            imp: m,
            rand: RandomGenerator::new(),
        }
    }

    fn calc_allowed(&mut self, step: &Step) -> i64 {
        if self.rand.random_usize(100) < step.take {
            step.win
        } else {
            0
        }
    }

    fn run<F>(allowed: i64, m: &mut M, mut f: F) -> Result<GameData<<ProxyMath<M> as SlotMath>::Special, <ProxyMath<M> as SlotMath>::Restore>, ServerError>
    where
        F: FnMut(&mut M) -> Result<GameData<<ProxyMath<M> as SlotMath>::Special, <ProxyMath<M> as SlotMath>::Restore>, ServerError>,
    {
        let mut attempt = 0;
        let mut result = f(m)?;

        let (is_free, is_respin) = match &result {
            GameData::FreeSpin(_) => (true, false),
            GameData::ReSpin(_) => (false, true),
            _ => (false, false),
        };
        while result.total() > allowed && attempt < MAX_ATTEMPTS {
            let current = f(m)?;
            if is_respin && result.respins() >= current.respins() {
                result = current;
            } else if is_free {
                let initial_on = result.free().map(|f| f.initial).unwrap_or(0);
                let initial_to = current.free().map(|f| f.initial).unwrap_or(0);
                if !current.has_respin() && initial_on >= initial_to && result.total() > current.total() {
                    result = current;
                }
            } else {
                let is_replace = !current.has_respin() && current.free().map(|f| f.initial).unwrap_or(0) == 0;
                if (result.has_respin() || result.free().map(|f| f.initial).unwrap_or(0) > 0 || result.total() > current.total()) && is_replace {
                    result = current
                }
            }
            attempt += 1;
        }
        Ok(result)
    }
}

impl<M: SlotMath> SlotMath for ProxyMath<M> {
    type Special = M::Special;

    type Restore = M::Restore;

    type PlayFSM = M::PlayFSM;

    type Rand = M::Rand;

    type Input = M::Input;

    type Calculator = M::Calculator;

    type V = M::V;

    fn join(&self, arg: JoinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.imp.join(arg)
    }

    fn init(&mut self, arg: GameInitArg, actions: &Vec<fugaso_action::Model>) -> Result<(), ServerError> {
        self.imp.init(arg, actions)
    }

    fn spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let allowed = self.calc_allowed(step);
        let f = |m: &mut M| m.spin(request, arg.clone(), step, combo.clone());
        Self::run(allowed, &mut self.imp, f)
    }

    fn free_spin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let allowed = self.calc_allowed(step);
        let f = |m: &mut M| m.free_spin(request, arg.clone(), step, combo.clone());
        Self::run(allowed, &mut self.imp, f)
    }

    fn respin(&mut self, request: &Self::Input, arg: SpinArg, step: &Step, combo: Option<Vec<usize>>) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        let allowed = self.calc_allowed(step);
        let f = |m: &mut M| m.respin(request, arg.clone(), step, combo.clone());
        Self::run(allowed, &mut self.imp, f)
    }

    fn post_process(&mut self, kind: ActionKind, game_data: GameData<Self::Special, Self::Restore>) -> Result<Arc<GameData<Self::Special, Self::Restore>>, ServerError> {
        self.imp.post_process(kind, game_data)
    }

    fn close(&self, next_act: ActionKind) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.imp.close(next_act)
    }

    fn collect(&self, request: &Self::Input, arg: SpinArg) -> Result<GameData<Self::Special, Self::Restore>, ServerError> {
        self.imp.collect(request, arg)
    }

    fn create_bet_calculator(&self) -> Self::Calculator {
        self.imp.create_bet_calculator()
    }

    fn create_fsm(&self, game_name: &str) -> Self::PlayFSM {
        self.imp.create_fsm(game_name)
    }

    fn settings(&self) -> MathSettings {
        self.imp.settings()
    }

    fn set_rand(&mut self, rand: Self::Rand) {
        self.imp.set_rand(rand)
    }
}

pub struct ReplayMath<M: SlotMath> {
    phantom: PhantomData<M>,
    game_datas: Vec<Arc<GameData<M::Special, M::Restore>>>,
    pos: usize,
}

impl<M: SlotMath> ReplayMath<M> {
    pub fn new() -> Self {
        Self {
            phantom: PhantomData,
            game_datas: vec![],
            pos: 0,
        }
    }
    pub fn load(&mut self, round: fugaso_round::Model, actions: Vec<fugaso_action::Model>) -> Result<Decimal, ServerError> {
        let common_id = round.common_id.ok_or_else(|| err_on!("common_ id is none!"))?;
        let balance = round.balance.ok_or_else(|| err_on!("balance is none!"))?;
        let stake = round.stake.ok_or_else(|| err_on!("stake is none!"))?;
        let win = round.win.ok_or_else(|| err_on!("round win is none!"))?;

        let mut balance_start = (balance * Decimal::new(100, 0)).to_i64().ok_or_else(|| err_on!("balance is none!"))? + stake;
        if round.status == Some(fugaso_round::RoundStatus::SUCCESS) {
            if balance_start < win {
                warn!("balance < win - balance={balance_start} win={win}");
            }
            balance_start = std::cmp::max(0, balance_start - win);
        }
        let curr_reels = round.reels.map(|r| r as usize).ok_or_else(|| err_on!("reels are none!"))?;

        self.game_datas = actions
            .into_iter()
            .map(|a| {
                if a.act_descr == Some(ActionKind::BET)
                    || a.act_descr == Some(ActionKind::RESPIN)
                    || a.act_descr == Some(ActionKind::FREE_SPIN)
                    || a.act_descr == Some(ActionKind::COLLECT)
                {
                    let mut next_act = a.next_act.clone().ok_or_else(|| err_on!("next action is none!"))?;
                    let result: GameResult<M::Special, M::Restore> = GameResult::from_action(&a)?;
                    let free = a.free_games.map_or_else(|| Ok(None), |s| FreeGame::from_db(&s).map(|f| Some(f)))?;
                    let win_add = if a.act_descr == Some(ActionKind::COLLECT) {
                        win
                    } else {
                        0
                    };
                    if next_act == ActionKind::CLOSE {
                        next_act = ActionKind::BET
                    }
                    let spin_data = SpinData {
                        id: id::GAME_DATA,
                        balance: balance_start - stake + win_add,
                        credit_type: 100,
                        result: GameResult {
                            total: result.total,
                            stops: result.stops,
                            holds: result.holds,
                            grid: result.grid,
                            special: result.special,
                            gains: result.gains,
                            restore: result.restore,
                            ..Default::default()
                        },
                        curr_lines: round.line as usize,
                        curr_bet: round.bet,
                        curr_denom: round.denom,
                        curr_reels,
                        next_act,
                        category: a.reel_combo as usize,
                        round_id: common_id,
                        round_type: round.detail.clone(),
                        round_multiplier: round.multi,
                        free,
                        ..Default::default()
                    };
                    if a.act_descr == Some(ActionKind::BET) {
                        Ok(Some(GameData::Spin(spin_data)))
                    } else if a.act_descr == Some(ActionKind::RESPIN) {
                        Ok(Some(GameData::ReSpin(spin_data)))
                    } else if a.act_descr == Some(ActionKind::FREE_SPIN) {
                        Ok(Some(GameData::FreeSpin(spin_data)))
                    } else if a.act_descr == Some(ActionKind::COLLECT) {
                        Ok(Some(GameData::Collect(spin_data)))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, ServerError>>()?
            .into_iter()
            .filter_map(|v| v)
            .map(|v| Arc::new(v))
            .collect::<Vec<_>>();
        debug!("game_datas: {}", self.game_datas.len());
        Ok(Decimal::new(balance_start, 2))
    }

    pub fn next(&mut self) -> Result<Arc<GameData<M::Special, M::Restore>>, ServerError> {
        if self.game_datas.is_empty() {
            return Err(err_on!("game data is empty!"));
        }
        let next = Arc::clone(&self.game_datas[self.pos]);
        self.pos = (self.pos + 1) % self.game_datas.len();
        Ok(next)
    }
}
