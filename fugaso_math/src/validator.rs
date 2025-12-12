use crate::math::{GameInput, MathSettings, Request};
use essential_core::{
    err_on,
    error::{message::ILLEGAL_ARGUMENT, ServerError},
};
use fugaso_data::{fugaso_percent, fugaso_round};
use std::cmp::min;

#[derive(Debug, Default)]
pub struct RequestValidator {
    pub lines: Vec<usize>,
    pub bets: Vec<i32>,
    pub denomination: Vec<i32>,
    pub reels: Vec<usize>,
    pub bet_counters: Vec<usize>,
}

impl RequestValidator {
    pub const MIDDLE: usize = 4;

    fn correct_val<T: Copy + PartialEq + PartialOrd>(values: &Vec<T>, value: T) -> T {
        if values.contains(&value) {
            value
        } else if values[0] > value {
            values[0]
        } else {
            values[values.len() - 1]
        }
    }
}

pub trait SimpleValidator {
    fn get_bet_index(&self, v: i32) -> usize;

    fn get_index<T: Copy + PartialEq + PartialOrd>(values: &Vec<T>, v: T) -> usize;

    fn max_line(&self) -> usize;

    fn min_bet_counter(&self) -> usize;

    fn min_reels(&self) -> usize;

    fn lines(&self) -> Vec<usize>;

    fn bets(&self) -> Vec<i32>;

    fn denomination(&self) -> Vec<i32>;

    fn reels(&self) -> Vec<usize>;

    fn bet_counters(&self) -> Vec<usize>;
}

pub trait Validator: Sized {
    type I;

    fn new(percent: &fugaso_percent::Model, settings: MathSettings) -> Result<Self, ServerError>;

    fn from_round(&self, r: &fugaso_round::Model, reels_default: usize) -> Self::I;

    fn correct(&self, req: &mut Self::I);

    fn get_default_request(&self, default_index: usize, default_line: Option<usize>) -> Self::I;

    fn get_promo_request(&self, stake: i64, stakes: Vec<GameInput>)
        -> Result<Self::I, ServerError>;
}

impl Validator for RequestValidator {
    type I = Request;

    fn new(percent: &fugaso_percent::Model, settings: MathSettings) -> Result<Self, ServerError> {
        let bet_st = percent
            .poss_bets
            .as_ref()
            .ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
        let d_st = percent
            .denomination
            .as_ref()
            .ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
        let bets_on = bet_st
            .split(",")
            .map(|s| s.parse::<i32>())
            .collect::<Result<Vec<_>, _>>()?;
        let denom_on = d_st
            .split(",")
            .map(|s| s.parse::<i32>())
            .collect::<Result<Vec<_>, _>>()?;

        if settings.lines.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }
        if bets_on.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }
        if denom_on.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }
        if settings.bet_counters.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }
        if settings.reels.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }

        Ok(Self {
            lines: settings.lines,
            bets: bets_on,
            denomination: denom_on,
            reels: settings.reels,
            bet_counters: settings.bet_counters,
        })
    }

    fn from_round(&self, r: &fugaso_round::Model, reels_default: usize) -> Self::I {
        Request {
            bet: r.bet,
            line: r.line as usize,
            denom: r.denom,
            bet_index: self.get_bet_index(r.bet),
            bet_counter: r.bet_counter as usize,
            reels: r.reels.map(|v| v as usize).unwrap_or(reels_default),
        }
    }

    fn correct(&self, req: &mut Self::I) {
        req.line = Self::correct_val(&self.lines, req.line);
        req.denom = Self::correct_val(&self.denomination, req.denom);
        req.bet = Self::correct_val(&self.bets, req.bet);
        req.reels = Self::correct_val(&self.reels, req.reels);
        req.bet_counter = Self::correct_val(&self.bet_counters, req.bet_counter);
        req.bet_index = Self::get_index(&self.bets, req.bet);
    }

    fn get_default_request(&self, default_index: usize, default_line: Option<usize>) -> Self::I {
        let bet;
        let bet_index;
        let denom;
        if self.denomination.len() > 2 {
            bet = self.bets[0];
            bet_index = Self::get_index(&self.bets, bet);
            denom = self.denomination[min(default_index, self.denomination.len() - 1)];
        } else {
            bet = self.bets[min(default_index, self.bets.len() - 1)];
            bet_index = Self::get_index(&self.bets, bet);
            denom = self.denomination[0];
        }
        let line = if let Some(i) = default_line {
            self.lines[i]
        } else {
            self.lines[self.lines.len() - 1]
        };
        let reels = self.reels[self.reels.len() - 1];

        return Request {
            bet,
            line,
            denom,
            bet_index,
            bet_counter: 1,
            reels,
        };
    }

    fn get_promo_request(
        &self,
        stake: i64,
        stakes: Vec<GameInput>,
    ) -> Result<Self::I, ServerError> {
        if stakes.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }
        let game_input = stakes
            .iter()
            .find(|s| s.stake >= stake)
            .unwrap_or(&stakes[stakes.len() - 1]);
        Ok(Request {
            bet: game_input.bet,
            line: game_input.line,
            denom: game_input.denom,
            bet_index: self.get_bet_index(game_input.bet),
            bet_counter: self.min_bet_counter(),
            reels: self.min_reels(),
        })
    }
}

impl SimpleValidator for RequestValidator {
    fn get_bet_index(&self, v: i32) -> usize {
        Self::get_index(&self.bets, v)
    }

    fn get_index<T: Copy + PartialEq + PartialOrd>(values: &Vec<T>, v: T) -> usize {
        values.iter().position(|e| *e == v).unwrap_or(0)
    }

    fn max_line(&self) -> usize {
        self.lines[self.lines.len() - 1]
    }

    fn min_bet_counter(&self) -> usize {
        self.bet_counters[0]
    }

    fn min_reels(&self) -> usize {
        self.reels[0]
    }

    fn lines(&self) -> Vec<usize> {
        self.lines.clone()
    }

    fn bets(&self) -> Vec<i32> {
        self.bets.clone()
    }

    fn denomination(&self) -> Vec<i32> {
        self.denomination.clone()
    }

    fn reels(&self) -> Vec<usize> {
        self.reels.clone()
    }

    fn bet_counters(&self) -> Vec<usize> {
        self.bet_counters.clone()
    }
}
