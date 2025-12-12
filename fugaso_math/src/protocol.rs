use crate::math::IPlayResponse;
use essential_async::channel::UnboundedSender;
use essential_core::err_on;
use essential_core::error::message::ILLEGAL_ARGUMENT;
use essential_core::error::ServerError;
use fugaso_data::fugaso_action;
use fugaso_data::fugaso_action::ActionKind;
use fugaso_data::fugaso_action::ActionKind::RESPIN;
use fugaso_data::fugaso_round::RoundDetail;
use serde::ser::SerializeSeq;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

#[derive(Debug)]
pub enum GamePlayEvent {
    Overlay,
    Stop,
    Result(PlayEventData),
}

#[derive(Debug)]
pub struct PlayEventData {
    pub gains: Vec<Gain>,
    pub category: usize,
    pub next_action: ActionKind,
    pub current: ActionKind,
    pub stop: i32,
}

pub mod id {
    pub const LOGIN: i32 = 8;
    pub const JOIN: i32 = 7;
    pub const GAME_DATA: i32 = 6;
    pub const TOURNAMENT_INFO: i32 = 261;
    pub const HISTORY: i32 = 256;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "subType")]
pub enum GameData<S: DatabaseStore + Default, R: Default> {
    #[serde(rename = "INITIAL")]
    Initial(InitialData<S, R>),
    #[serde(rename = "SPIN")]
    Spin(SpinData<S, R>),
    #[serde(rename = "RESPIN")]
    ReSpin(SpinData<S, R>),
    #[serde(rename = "COLLECT")]
    Collect(SpinData<S, R>),
    #[serde(rename = "FREE_SPIN")]
    FreeSpin(SpinData<S, R>),
}

impl<S: DatabaseStore + Default + 'static, R: Default + 'static> Deref for GameData<S, R> {
    type Target = dyn IPlayResponse;

    fn deref(&self) -> &Self::Target {
        match self {
            GameData::Initial(v) => v,
            GameData::Spin(v) => v,
            GameData::FreeSpin(v) => v,
            GameData::ReSpin(v) => v,
            GameData::Collect(v) => v,
        }
    }
}

impl<S: DatabaseStore + Default + 'static, R: Default + 'static> DerefMut for GameData<S, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            GameData::Initial(v) => v,
            GameData::ReSpin(v) => v,
            GameData::Spin(v) => v,
            GameData::FreeSpin(v) => v,
            GameData::Collect(v) => v,
        }
    }
}

impl<S: DatabaseStore + Default, R: Default> SpinData<S, R> {
    pub fn fire_event(&self, current: ActionKind, sender: &UnboundedSender<GamePlayEvent>) {
        sender.send(
            GamePlayEvent::Result(PlayEventData {
                gains: self.result.gains.clone(),
                category: self.category,
                next_action: self.next_act.clone(),
                current,
                stop: self.result.special.as_ref().map(|s| s.stop()).unwrap_or(0),
            }),
            file!(),
            line!(),
        );
    }
}

impl<S: DatabaseStore + Default, R: Default + 'static> IPlayResponse for SpinData<S, R> {
    fn create_action_default(&self) -> Result<fugaso_action::Model, ServerError> {
        Ok(fugaso_action::Model {
            next_combo: 0,
            custom: None,
            choice: None,
            cards: None,
            reel_combo: self.category as i32,
            free_games: if let Some(f) = self.free.as_ref() {
                Some(f.to_db()?)
            } else {
                None
            },
            ..self.result.to_action()?
        })
    }

    fn free(&self) -> Option<&FreeGame> {
        self.free.as_ref()
    }

    fn has_bonus(&self) -> bool {
        false
    }

    fn has_respin(&self) -> bool {
        self.next_act == RESPIN
    }

    fn has_drop(&self) -> bool {
        false
    }

    fn total(&self) -> i64 {
        self.result.total
    }

    fn respins(&self) -> i32 {
        self.result
            .special
            .as_ref()
            .map(|s| s.respins())
            .unwrap_or(0)
    }

    fn is_gamble_end(&self, _total_bet: i64) -> bool {
        false
    }

    fn stops_on(&self) -> Vec<usize> {
        self.result.stops.clone()
    }

    fn grid_on(&self) -> Vec<Vec<char>> {
        self.result.grid.clone()
    }

    fn promo(&self) -> Promo {
        self.promo.clone()
    }

    fn set_next_act(&mut self, kind: ActionKind) {
        self.next_act = kind;
    }
}

impl<S: DatabaseStore + Default, R: Default> IPlayResponse for InitialData<S, R> {
    fn create_action_default(&self) -> Result<fugaso_action::Model, ServerError> {
        Ok(fugaso_action::Model {
            next_combo: 0,
            custom: None,
            choice: None,
            cards: None,
            reel_combo: self.category as i32,
            free_games: if let Some(f) = self.free.as_ref() {
                Some(f.to_db()?)
            } else {
                None
            },
            ..self.result.to_action()?
        })
    }

    fn free(&self) -> Option<&FreeGame> {
        None
    }

    fn has_bonus(&self) -> bool {
        false
    }

    fn has_respin(&self) -> bool {
        false
    }

    fn has_drop(&self) -> bool {
        false
    }

    fn total(&self) -> i64 {
        0
    }

    fn is_gamble_end(&self, _total_bet: i64) -> bool {
        false
    }

    fn stops_on(&self) -> Vec<usize> {
        self.result.stops.clone()
    }

    fn grid_on(&self) -> Vec<Vec<char>> {
        self.result.grid.clone()
    }

    fn promo(&self) -> Promo {
        self.promo.clone()
    }

    fn set_next_act(&mut self, kind: ActionKind) {
        self.next_act = kind;
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitialData<S: DatabaseStore + Default, R: Default> {
    pub id: i32,
    pub balance: i64,
    pub credit_type: i32,
    pub min_bet: i32,
    pub max_bet: i32,
    pub lines: Vec<Vec<usize>>,
    #[serde(
        deserialize_with = "deserialize_vec_reels",
        serialize_with = "serialize_vec_reels"
    )]
    pub reels: Vec<Vec<Vec<char>>>,
    pub poss_lines: Vec<usize>,

    pub result: GameResult<S, R>,
    pub poss_bets: Vec<i32>,
    #[serde(rename = "possDenoms")]
    pub poss_denom: Vec<i32>,
    pub poss_reels: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub poss_bet_counters: Vec<usize>,
    pub curr_lines: usize,
    pub curr_bet: i32,
    pub curr_denom: i32,
    pub curr_reels: usize,
    pub bet_counter: usize,
    pub wins: Vec<Win>,
    pub next_act: ActionKind,
    pub category: usize,
    pub round_id: i64,
    pub round_type: RoundDetail,
    pub round_multiplier: i32,
    pub promo: Promo,
    pub free: Option<FreeGame>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpinData<S: DatabaseStore + Default, R: Default> {
    pub id: i32,
    pub balance: i64,
    pub credit_type: i32,
    pub result: GameResult<S, R>,
    pub curr_lines: usize,
    pub curr_bet: i32,
    pub curr_denom: i32,
    pub curr_reels: usize,
    pub next_act: ActionKind,
    pub category: usize,
    pub round_id: i64,
    pub round_type: RoundDetail,
    pub round_multiplier: i32,
    pub promo: Promo,
    pub free: Option<FreeGame>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FreeGame {
    pub total_win: i64,
    pub symbol: char,
    pub category: usize,

    pub initial: i32,
    pub left: i32,
    pub done: i32,
}

impl Default for FreeGame {
    fn default() -> Self {
        Self {
            total_win: 0,
            symbol: '?',
            category: 0,
            initial: 0,
            left: 0,
            done: 0,
        }
    }
}

impl FreeGame {
    pub fn add(&mut self, games: i32) {
        self.initial += games;
        self.left += games;
    }

    pub fn play(&mut self) {
        self.left -= 1;
        self.done += 1
    }
}

impl DatabaseStore for FreeGame {
    fn from_db(value: &str) -> Result<Self, ServerError>
    where
        Self: Sized,
    {
        let split = value.split("|");
        let mut free_game = FreeGame::default();
        for s in split {
            let p = s.split("=").collect::<Vec<_>>();
            if p.len() > 1 {
                if "left" == p[0] {
                    free_game.left = p[1].parse::<i32>().map_err(|e| err_on!(e.to_string()))?;
                } else if "done" == p[0] {
                    free_game.done = p[1].parse::<i32>().map_err(|e| err_on!(e.to_string()))?;
                } else if "initial" == p[0] {
                    free_game.initial = p[1].parse::<i32>().map_err(|e| err_on!(e.to_string()))?;
                } else if "symbol" == p[0] {
                    free_game.symbol = p[1]
                        .chars()
                        .nth(0)
                        .ok_or_else(|| err_on!("symbol is none!"))?;
                } else if "totalWin" == p[0] {
                    free_game.total_win =
                        p[1].parse::<i64>().map_err(|e| err_on!(e.to_string()))?;
                } else if "category" == p[0] {
                    free_game.category =
                        p[1].parse::<usize>().map_err(|e| err_on!(e.to_string()))?;
                }
            } else {
                return Err(err_on!("error parse free_game!"));
            }
        }
        Ok(free_game)
    }

    fn to_db(&self) -> Result<String, ServerError> {
        Ok(format!(
            "left={}|done={}|initial={}|symbol={}|totalWin={}|category={}",
            self.left, self.done, self.initial, self.symbol, self.total_win, self.category
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Promo {
    pub amount: i32,
    pub multi: i32,
}

impl Default for Promo {
    fn default() -> Self {
        Self {
            amount: 0,
            multi: 1,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameResult<S: DatabaseStore + Default, R: Default> {
    pub total: i64,
    pub stops: Vec<usize>,
    pub holds: Vec<i32>,
    pub cards: Vec<Card>,
    pub grid: Vec<Vec<char>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special: Option<S>,
    #[serde(rename = "wons")]
    pub gains: Vec<Gain>,
    #[serde(skip)]
    pub restore: Option<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_data: Option<R>,
}

impl<S: DatabaseStore + Default, R: Default> GameResult<S, R> {
    pub fn from_action(action: &fugaso_action::Model) -> Result<Self, ServerError> {
        Ok(GameResult {
            total: action.amount,
            stops: if let Some(s) = &action.reel_stops {
                s.split(",")
                    .map(|s| s.parse::<usize>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| err_on!(e))?
            } else {
                vec![]
            },
            holds: if let Some(s) = &action.holds {
                s.split(",")
                    .map(|s| s.parse::<i32>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| err_on!(e))?
            } else {
                vec![]
            },
            cards: vec![],
            grid: if let Some(s) = &action.grid {
                s.split(";")
                    .map(|v| {
                        v.split(",")
                            .map(|v| v.chars().nth(0).ok_or_else(|| err_on!(ILLEGAL_ARGUMENT)))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                vec![]
            },
            special: if let Some(s) = &action.special {
                Some(S::from_db(&s)?)
            } else {
                None
            },
            gains: if let Some(l) = &action.lines {
                let gains = l
                    .split(";")
                    .map(|s| Gain::from_db(s))
                    .collect::<Result<Vec<_>, _>>();
                match gains {
                    Ok(v) => v,
                    Err(_) => {
                        vec![]
                    }
                }
            } else {
                vec![]
            },
            restore: None,
            extra_data: None,
            ..Default::default()
        })
    }

    pub fn to_action(&self) -> Result<fugaso_action::Model, ServerError> {
        Ok(fugaso_action::Model {
            amount: self.total,
            special: if let Some(s) = &self.special {
                Some(s.to_db()?)
            } else {
                None
            },
            reel_stops: Some(
                self.stops
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            grid: Some(
                self.grid
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            holds: Some(
                self.holds
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            lines: if self.gains.is_empty() {
                None
            } else {
                Some(
                    self.gains
                        .iter()
                        .map(|g| g.to_db())
                        .collect::<Result<Vec<_>, _>>()?
                        .join(";"),
                )
            },
            ..Default::default()
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReSpinInfo {
    #[serde(default)]
    pub total: i64,
    pub multipliers: Vec<i32>,
    pub respins: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
    #[serde(default)]
    pub accum: i64,
}

impl DatabaseStore for ReSpinInfo {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        serde_json::from_str(&value).map_err(|e| err_on!(e))
    }

    fn to_db(&self) -> Result<String, ServerError> {
        serde_json::to_string(self).map_err(|e| err_on!(e))
    }

    fn fire_event(&self, sender: &UnboundedSender<GamePlayEvent>) {
        if self.overlay.is_some() {
            sender.send(GamePlayEvent::Overlay, file!(), line!())
        }
    }

    fn respins(&self) -> i32 {
        self.respins
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RestoreInfo {
    pub multipliers: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<Vec<Vec<char>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartInfo {
    pub mults: Vec<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<Vec<Vec<char>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Vec<Vec<char>>>,
}

impl DatabaseStore for OverlayInfo {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        serde_json::from_str(&value).map_err(|e| err_on!(e))
    }

    fn to_db(&self) -> Result<String, ServerError> {
        serde_json::to_string(self).map_err(|e| err_on!(e))
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub index: i32,
}

impl DatabaseStore for IndexInfo {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        serde_json::from_str(&value).map_err(|e| err_on!(e))
    }

    fn to_db(&self) -> Result<String, ServerError> {
        serde_json::to_string(self).map_err(|e| err_on!(e))
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiplierInfo {
    pub multi: usize,
    pub feature: char,
}

impl DatabaseStore for MultiplierInfo {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        serde_json::from_str(&value).map_err(|e| err_on!(e))
    }

    fn to_db(&self) -> Result<String, ServerError> {
        serde_json::to_string(self).map_err(|e| err_on!(e))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub symbol: String,
    pub color: String,
    pub symbol_string: String,
    pub suit: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Win {
    pub symbol: char,
    pub count: usize,
    pub factor: i32,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Gain {
    pub symbol: char,
    pub count: usize,
    pub offset: i32,
    pub amount: i64,
    pub line_num: usize,
    pub multi: i32,
    pub free_spins: i32,
    #[serde(skip)]
    pub columns: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<BasePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Copy, PartialEq, Eq, Hash)]
pub struct BasePoint {
    pub x: usize,
    pub y: usize,
}

pub trait DatabaseStore {
    fn from_db(value: &str) -> Result<Self, ServerError>
    where
        Self: Sized;
    fn to_db(&self) -> Result<String, ServerError>;

    #[allow(unused_variables)]
    fn fire_event(&self, sender: &UnboundedSender<GamePlayEvent>) {}

    fn respins(&self) -> i32 {
        0
    }

    fn stop(&self) -> i32 {
        0
    }
}

impl DatabaseStore for Gain {
    fn from_db(value: &str) -> Result<Self, ServerError> {
        let parts = value.split(",").collect::<Vec<_>>();
        let err_index = || err_on!(ILLEGAL_ARGUMENT);
        let (columns, indexes, points) = if let Some(c) = parts.get(7) {
            let columns = c.parse::<usize>().map_err(|e| err_on!(e))?;
            let indexes = parts
                .get(8)
                .map(|s| {
                    s.split("|")
                        .map(|s| s.parse::<usize>())
                        .collect::<Result<Vec<_>, _>>()
                })
                .unwrap_or(Ok(vec![]))?;
            let points = indexes
                .iter()
                .map(|i| {
                    let y = i / columns;
                    let x = i - y * columns;
                    BasePoint { x, y }
                })
                .collect();
            (Some(columns), indexes, points)
        } else {
            (None, vec![], vec![])
        };
        Ok(Self {
            symbol: parts
                .get(0)
                .ok_or_else(err_index)?
                .chars()
                .nth(0)
                .ok_or_else(err_index)?,
            line_num: parts
                .get(1)
                .ok_or_else(err_index)?
                .parse::<usize>()
                .map_err(|e| err_on!(e))?,
            count: parts
                .get(2)
                .ok_or_else(err_index)?
                .parse::<usize>()
                .map_err(|e| err_on!(e))?,
            amount: parts
                .get(3)
                .ok_or_else(err_index)?
                .parse::<i64>()
                .map_err(|e| err_on!(e))?,
            offset: parts
                .get(4)
                .ok_or_else(err_index)?
                .parse::<i32>()
                .map_err(|e| err_on!(e))?,
            multi: parts
                .get(5)
                .ok_or_else(err_index)?
                .parse::<i32>()
                .map_err(|e| err_on!(e))?,
            free_spins: parts
                .get(6)
                .ok_or_else(err_index)?
                .parse::<i32>()
                .map_err(|e| err_on!(e))?,
            columns,
            indexes,
            points,
            ..Default::default()
        })
    }

    fn to_db(&self) -> Result<String, ServerError> {
        match self.columns {
            Some(c) => Ok(format!(
                "{},{},{},{},{},{},{},{},{}",
                self.symbol,
                self.line_num,
                self.count,
                self.amount,
                self.offset,
                self.multi,
                self.free_spins,
                c,
                self.indexes
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join("|")
            )),
            None => Ok(format!(
                "{},{},{},{},{},{},{}",
                self.symbol,
                self.line_num,
                self.count,
                self.amount,
                self.offset,
                self.multi,
                self.free_spins
            )),
        }
    }

    fn fire_event(&self, _sender: &UnboundedSender<GamePlayEvent>) {}
}

pub fn deserialize_lines<'de, D>(deserializer: D) -> Result<Vec<Vec<usize>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Vec<&str> = de::Deserialize::deserialize(deserializer)?;
    let values: Vec<_> = s
        .iter()
        .map(|s| {
            s.chars()
                .map(|c| match c {
                    '^' => Ok(0),
                    '-' => Ok(1),
                    '_' => Ok(2),
                    'V' => Ok(3),
                    _ => Err(de::Error::custom("error line symbol!")),
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

pub fn deserialize_vec_reels<'de, D>(deserializer: D) -> Result<Vec<Vec<Vec<char>>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Vec<Vec<&str>> = de::Deserialize::deserialize(deserializer)?;
    let values: Vec<_> = s
        .iter()
        .map(|v| v.iter().map(|s| s.chars().map(|c| c).collect()).collect())
        .collect();
    Ok(values)
}

pub fn serialize_vec_reels<S, T: ToString + Serialize>(
    v: &Vec<Vec<Vec<T>>>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq: S::SerializeSeq = s.serialize_seq(Some(v.len()))?;
    for e in v {
        let items = e
            .iter()
            .map(|v| v.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(""))
            .collect::<Vec<_>>();
        seq.serialize_element(&items)?;
    }
    seq.end()
}
