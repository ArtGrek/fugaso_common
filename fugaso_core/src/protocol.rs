use std::collections::HashMap;
use std::fmt::Display;
use std::marker::PhantomData;
use essential_core::account_service::{AccountError, ErrorType, ProxyAlias};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use fugaso_math::protocol::{DatabaseStore, FreeGame, Gain, GameData, GamePlayEvent, GameResult};
use std::str::FromStr;
use std::sync::Arc;
use chrono::FixedOffset;
use essential_async::channel::UnboundedSender;
use essential_core::err_on;
use essential_core::error::ServerError;
use sea_orm::prelude::{DateTimeWithTimeZone, Decimal};
use fugaso_math::math::Request;
use log::error;
use sea_orm::FromQueryResult;
use fugaso_data::fugaso_round::RoundDetail;
use sea_orm::prelude::DateTime;
use serde::ser::SerializeSeq;
use fugaso_data::{fugaso_action, fugaso_round};
use fugaso_data::fugaso_action::ActionKind;
use crate::proxy::TournamentTransfer;
use crate::tournament::{TournamentInfo, TournamentPlace};

pub trait IResponse {
    fn render(&self) -> Result<Vec<u8>, serde_json::Error>;

    fn commit(&self, sender: &UnboundedSender<GamePlayEvent>) -> Result<ActionKind, ServerError>;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Response<S: DatabaseStore + Default + Serialize + Sync + Send, R: Default> {
    #[serde(rename = "LOGIN")]
    Login(LoginData),
    #[serde(rename = "CURRENCY")]
    Currency(CurrencyData),
    #[serde(rename = "JOIN")]
    Join(JoinData),
    #[serde(rename = "GAME_DATA")]
    GameData(Arc<GameData<S, R>>),
    #[serde(rename = "JACKPOTS_WIN")]
    JackpotsWin(JackpotsWinData),
    #[serde(rename = "ERROR")]
    Error(ErrorData),
    #[serde(rename = "TOURNAMENT_INFO")]
    TournamentInfo(TournamentData),
    #[serde(rename = "HISTORY")]
    History(HistoryData<S, R>),
    #[serde(rename = "TOURNAMENT_WIN")]
    TournamentWin(TournamentGainData),
}

impl<S: DatabaseStore + Default + Serialize + Sync + Send, R: Default + Serialize> IResponse for Vec<Response<S, R>> {
    fn render(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    fn commit(&self, sender: &UnboundedSender<GamePlayEvent>) -> Result<ActionKind, ServerError> {
        let mut result = Err(err_on!("Next action is unknown!"));
        for r in self {
            match r {
                Response::GameData(v) => {
                    match v.as_ref() {
                        GameData::Initial(_) => { return Err(err_on!("Wrong action play!")); }
                        GameData::Spin(d) => {
                            if let Some(s) = d.result.special.as_ref() {
                                s.fire_event(&sender);
                            }
                            d.fire_event(ActionKind::SPIN, &sender);
                            result = Ok(d.next_act.clone())
                        }
                        GameData::FreeSpin(d) => {
                            if let Some(s) = d.result.special.as_ref() {
                                s.fire_event(&sender);
                            }
                            d.fire_event(ActionKind::FREE_SPIN, &sender);
                            result = Ok(d.next_act.clone())
                        }
                        GameData::ReSpin(d) => { 
                            d.fire_event(ActionKind::RESPIN, &sender);
                            result = Ok(d.next_act.clone())
                         }
                        GameData::Collect(d) => { 
                            d.fire_event(ActionKind::COLLECT, &sender);
                            result = Ok(d.next_act.clone()) 
                        }
                    }
                }
                Response::Error(e) => return Err(err_on!(e.message)),
                _ => {}
            }
        }
        result
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentGainData {
    pub id: i64,
    pub amount: Decimal,
    pub winners: Arc<Vec<TournamentPlace>>,
    pub balance: i64,
    pub place: i32,
    pub name: String,
    #[serde(with = "rust_decimal::serde::float")]
    pub balance_tour: Decimal,
}

impl From<TournamentTransfer> for TournamentGainData {
    fn from(value: TournamentTransfer) -> Self {
        Self {
            id: -1,
            amount: value.amount,
            winners: value.winners,
            balance: value.balance,
            place: value.place,
            name: value.name,
            balance_tour: value.points,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryData<S: DatabaseStore + Default, R: Default> {
    pub id: i32,
    pub rounds: Vec<RoundStory<S, R>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundStory<S: DatabaseStore + Default, R: Default> {
    pub bet: Option<i64>,
    pub game_name: Option<String>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub balance: Option<Decimal>,
    #[serde(serialize_with = "serialize_op_ts_milliseconds")]
    pub date_start: Option<DateTime>,
    pub kind: RoundDetail,
    pub id: i64,
    pub reels: Option<i32>,
    pub win: Option<i64>,
    pub actions: Vec<ActionStory<S, R>>,
    pub stake_on: i64,
    pub bet_counter: i32,
    pub multiplier: i32,
    pub line: i32,
    pub denom: i32,
    pub bet_on: i32,
}

impl<S: DatabaseStore + Default, R: Default> From<(fugaso_round::Model, Vec<fugaso_action::Model>)> for RoundStory<S, R> {
    fn from(value: (fugaso_round::Model, Vec<fugaso_action::Model>)) -> Self {
        Self {
            bet: value.0.stake,
            game_name: None,
            balance: value.0.balance.map(|b| b * Decimal::new(100, 0)),
            date_start: value.0.timestamp_open,
            kind: value.0.detail,
            id: value.0.common_id.unwrap_or(value.0.id),
            reels: value.0.reels,
            win: value.0.win,
            stake_on: 0,
            actions: value.1.into_iter().map(|a| a.into()).collect(),
            bet_counter: value.0.bet_counter,
            multiplier: value.0.multi,
            line: value.0.line,
            denom: value.0.denom,
            bet_on: value.0.bet,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStory<S: DatabaseStore + Default, R: Default> {
    pub id: i64,
    pub special: Option<S>,
    pub cards: Option<String>,
    pub description: Option<ActionKind>,
    pub holds: Vec<i32>,
    #[serde(rename = "wons")]
    pub gains: Vec<Gain>,
    pub stops: Option<String>,
    pub category: i32,
    pub choice: Option<String>,
    pub grid: Vec<Vec<char>>,
    pub tournaments: Vec<TournamentUserWin>,
    #[serde(skip)]
    pub restore: PhantomData<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free: Option<FreeGame>,
}

impl<S: DatabaseStore + Default, R: Default> From<fugaso_action::Model> for ActionStory<S, R> {
    fn from(value: fugaso_action::Model) -> Self {
        let result: GameResult<S, R> = match GameResult::from_action(&value) {
            Ok(r) => { r }
            Err(e) => {
                error!("error game result from action {e}!");
                GameResult::default()
            }
        };
        Self {
            id: value.id,
            special: result.special,
            cards: value.cards,
            description: value.act_descr,
            holds: result.holds,
            gains: result.gains,
            stops: value.reel_stops,
            category: value.reel_combo,
            choice: value.choice,
            grid: result.grid,
            tournaments: vec![],
            free: if let Some(f) = value.free_games {
                match FreeGame::from_db(&f) {
                    Ok(f) => { Some(f) }
                    Err(e) => {
                        error!("{e}");
                        None
                    }
                }
            } else {
                None
            },
            restore: PhantomData,
        }
    }
}

#[derive(Debug, FromQueryResult, Default, Serialize, Deserialize)]
pub struct TournamentUserWin {
    pub id: i64,
    #[serde(rename = "name")]
    pub tour: String,
    #[serde(with = "rust_decimal::serde::float")]
    pub amount: Decimal,
    #[serde(rename = "commonId")]
    pub round_id: String,
    #[serde(rename = "date", serialize_with = "serialize_ts_milliseconds")]
    pub time_done: DateTime,
    pub place: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentData {
    pub id: i32,
    pub tournament: TournamentInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorData {
    pub message: String,
    pub code: ErrorType,
}

impl From<AccountError> for ErrorData {
    fn from(value: AccountError) -> Self {
        let (kind, message) = value.kind();
        Self {
            message,
            code: kind,
        }
    }
}

impl From<ServerError> for ErrorData {
    fn from(value: ServerError) -> Self {
        error!("{value:?}");
        Self {
            message: "Unknown error.".to_string(),
            code: ErrorType::UNKNOWN,
        }
    }
}

#[derive(Debug)]
pub enum PlayerError {
    Internal(ServerError),
    Account(AccountError),
    Admin(AdminError),
}

#[derive(Debug)]
pub struct AdminError {
    pub action_id: i64,
    pub round_id: i64,
    pub error: AccountError,
}

impl From<AccountError> for PlayerError {
    fn from(value: AccountError) -> Self {
        PlayerError::Account(value)
    }
}

impl From<ServerError> for PlayerError {
    fn from(value: ServerError) -> Self {
        PlayerError::Internal(value)
    }
}

impl From<AdminError> for PlayerError {
    fn from(value: AdminError) -> Self {
        PlayerError::Admin(value)
    }
}


#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginData {
    pub id: i32,
    pub game_id: i64,
    pub game_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyData {
    pub id: i64,
    pub code: Option<String>,
    pub symbol: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinData {
    pub id: i32,
    #[serde(rename = "userid")]
    pub user_id: i64,
    pub nickname: String,
    #[serde(rename = "realcurrency")]
    pub currency: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JackpotsWinData {
    pub jackpots: HashMap<String, Decimal>,
    pub collected: Option<HashMap<String, Decimal>>,
    pub balance: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpinRequest {
    pub bet: i32,
    pub denom: i32,
    pub line: usize,
    #[serde(default)]
    pub bet_counter: usize,
}

impl From<SpinRequest> for Request {
    fn from(value: SpinRequest) -> Self {
        Self {
            bet: value.bet,
            denom: value.denom,
            line: value.line,
            bet_counter: value.bet_counter,
            bet_index: 0,
            reels: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeData {
    pub user_name: Option<String>,
    pub session_id: Option<String>,
    #[serde(deserialize_with = "from_str", serialize_with = "serialize_as_str")]
    pub operator_id: Option<i64>,
    pub game_name: String,
    pub mode: ProxyAlias,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    #[serde(deserialize_with = "from_json", serialize_with = "serialize_json")]
    pub session: AuthorizeData,
    pub password: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRequest {
    pub limit: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum PlayerRequest<R> {
    #[serde(rename = "LOGIN")]
    Login(LoginRequest),
    #[serde(rename = "BET_SPIN")]
    BetSpin(R),
    #[serde(rename = "COLLECT")]
    Collect,
    #[serde(rename = "RESPIN")]
    ReSpin,
    #[serde(rename = "FREE_SPIN")]
    FreeSpin,
    #[serde(rename = "TOURNAMENT_INFO")]
    TournamentInfo,
    #[serde(rename = "HISTORY")]
    History(HistoryRequest),
}

impl <R>PlayerRequest<R> {
    pub fn kind(&self) -> &'static str {
        match self {
            PlayerRequest::Login(_) => { "LOGIN" }
            PlayerRequest::BetSpin(_) => { "BET_SPIN" }
            PlayerRequest::Collect => { "COLLECT" }
            PlayerRequest::ReSpin => { "RESPIN" }
            PlayerRequest::FreeSpin => { "FREE_SPIN" }
            PlayerRequest::TournamentInfo => { "TOURNAMENT_INFO" }
            PlayerRequest::History(_) => { "HISTORY" }
        }
    }
}

fn from_json<'de, D>(deserializer: D) -> Result<AuthorizeData, D::Error>
    where
        D: Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(|e| de::Error::custom(e.to_string()))
}

pub fn from_str<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
    where T: FromStr,
          T::Err: Display,
          D: Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map(|v| Some(v)).map_err(de::Error::custom)
}

pub fn from_milliseconds_str<'de, D>(deserializer: D) -> Result<DateTimeWithTimeZone, D::Error>
    where D: Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    let value = i64::from_str(&s).map_err(de::Error::custom)?;
    chrono::DateTime::from_timestamp(
        (value / 1000) as i64,
        ((value % 1000) * 1_000_000) as u32,
    )
        .map(|dt| {
            let timezone_east = FixedOffset::east_opt(0).unwrap();
            DateTimeWithTimeZone::from_naive_utc_and_offset(dt.naive_utc(), timezone_east)
        })
        .ok_or_else(|| de::Error::custom(format!("error deserialize timestamp {}", value)))
}

pub fn serialize_date_time_ms<S>(v: &DateTimeWithTimeZone, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    s.serialize_some(&v.timestamp_millis())
}

pub fn serialize_op_ts_milliseconds<S>(v: &Option<DateTime>, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    match v {
        None => { s.serialize_none() }
        Some(t) => { s.serialize_some(&t.and_utc().timestamp_millis()) }
    }
}

pub fn serialize_json<S, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    use serde::ser::Error;
    let st = serde_json::to_string(v).map_err(|e| Error::custom(e.to_string()))?;
    s.serialize_str(&st)
}

pub fn serialize_as_str<S, T: ToString>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    match v {
        None => { s.serialize_none() }
        Some(d) => { s.serialize_str(&d.to_string()) }
    }
}

pub fn serialize_ts_milliseconds<S>(v: &DateTime, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    s.serialize_i64(v.and_utc().timestamp_millis())
}

pub fn serialize_vec_float<S>(v: &Vec<Decimal>, s: S) -> Result<S::Ok, S::Error> where S: Serializer
{
    use num_traits::ToPrimitive;
    use serde::ser::Error;
    let mut seq: S::SerializeSeq = s.serialize_seq(Some(v.len()))?;
    for e in v {
        let value = e.to_f64().ok_or_else(|| Error::custom("error decimal to f64 convert"))?;
        seq.serialize_element(&value)?;
    }
    seq.end()
}
