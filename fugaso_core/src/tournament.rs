use async_trait::async_trait;
use chrono::Local;
use essential_async::channel::{OneShotSender, UnboundedSender};
use essential_core::err_on;
use essential_core::error::message::ILLEGAL_ARGUMENT;
use essential_core::error::ServerError;
use essential_data::repo::UserAttributeRepository;
use essential_data::user_attribute::AttributeName;
use log::error;
use reqwest::{Client, StatusCode, Url};
use sea_orm::prelude::DateTimeWithTimeZone;
use std::sync::Arc;

use crate::protocol::TournamentUserWin;
use fugaso_data::repo::TournamentGainRepository;
use fugaso_data::tournament_gain;
use sea_orm::prelude::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentInfo {
    pub current: Option<TournamentState>,
    pub pending_wins: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentState {
    pub name: String,
    #[serde(deserialize_with = "crate::protocol::from_milliseconds_str", serialize_with = "crate::protocol::serialize_date_time_ms")]
    pub date_start: DateTimeWithTimeZone,
    #[serde(deserialize_with = "crate::protocol::from_milliseconds_str", serialize_with = "crate::protocol::serialize_date_time_ms")]
    pub date_end: DateTimeWithTimeZone,
    #[serde(with = "rust_decimal::serde::float")]
    pub min_bet: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub min_bet_euro: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub rate: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub share: Decimal,
    #[serde(default = "Vec::new")]
    pub places: Vec<TournamentPlace>,
    #[serde(serialize_with = "crate::protocol::serialize_vec_float")]
    pub rewards: Vec<Decimal>,
    pub position: Option<TournamentPosition>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentPlace {
    pub name: String,
    #[serde(with = "rust_decimal::serde::float")]
    pub balance: Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentPosition {
    index: i32,
    balance: Decimal,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TournamentConfig {
    pub url: String,
    pub ip: Arc<String>,
    pub name: Arc<String>,
    pub password: Arc<String>,
    pub logged: bool,
    pub server: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentIncreaseRequest {
    pub amount: Decimal,
    pub currency: String,
    pub ip: Arc<String>,
    pub stake: Decimal,
    pub tours: Arc<Vec<String>>,
    pub user_name: Arc<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TournamentAuthRequest {
    pub username: Arc<String>,
    pub password: Arc<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct JwtAuthentication {
    pub username: String,
    pub roles: Vec<String>,
    pub expires_in: i64,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
}

pub struct TournamentWinData {
    pub winners: Arc<Vec<TournamentPlace>>,
    pub gain: tournament_gain::Model,
    pub balance: Decimal,
    pub award_id: i64,
}

pub enum TournamentEvent {
    Auth(OneShotSender<String>),
    Login,
}

pub struct TournamentClient {
    http: Client,
    config: Arc<TournamentConfig>,
    auth: JwtAuthentication,
}

impl TournamentClient {
    pub async fn new(config: Arc<TournamentConfig>) -> Self {
        let http = Client::new();
        let logged = config.logged;
        let mut client = Self {
            http,
            config,
            auth: JwtAuthentication::default(),
        };
        if logged {
            match client.auth().await {
                Ok(a) => {
                    client.auth = a;
                }
                Err(e) => {
                    error!("{e:?}!");
                }
            };
        }

        client
    }
    pub async fn auth(&self) -> Result<JwtAuthentication, ServerError> {
        let url = Url::parse(&format!("{}/auth", self.config.url)).map_err(|e| err_on!(e))?;
        self.http
            .post(url)
            .json(&TournamentAuthRequest {
                username: self.config.name.clone(),
                password: self.config.password.clone(),
            })
            .send()
            .await
            .map_err(|e| err_on!(e))?
            .json::<JwtAuthentication>()
            .await
            .map_err(|e| err_on!(e))
    }
}

pub struct TournamentHolder {
    pub name: String,
    pub config: Arc<TournamentConfig>,
    sender: UnboundedSender<TournamentEvent>,
}

impl TournamentHolder {
    pub async fn new(config: Arc<TournamentConfig>, name: String) -> Self {
        let mut client = TournamentClient::new(Arc::clone(&config)).await;
        let (s, mut r) = mpsc::unbounded_channel::<TournamentEvent>();
        tokio::spawn(async move {
            while let Some(e) = r.recv().await {
                match e {
                    TournamentEvent::Login => {
                        match client.auth().await {
                            Ok(a) => {
                                client.auth = a;
                            }
                            Err(e) => {
                                error!("tournament auth error {e:?}!");
                            }
                        };
                    }
                    TournamentEvent::Auth(s) => {
                        s.send(client.auth.access_token.clone(), file!(), line!());
                    }
                }
            }
        });
        Self {
            config,
            name,
            sender: UnboundedSender::new(s),
        }
    }

    pub async fn login(&self) {
        self.sender.send(TournamentEvent::Login, file!(), line!());
    }

    pub async fn access_token(&self) -> String {
        let (s, r) = oneshot::channel();
        self.sender.send(TournamentEvent::Auth(OneShotSender::new(s)), file!(), line!());
        match r.await {
            Ok(t) => t,
            Err(_) => {
                error!("error receive token!");
                "".to_string()
            }
        }
    }

    pub fn ident(&self) -> String {
        self.config.server.clone().unwrap_or(self.name.clone())
    }
}

#[async_trait]
pub trait TournamentService {
    fn action(&self, amount: Decimal, currency_code: &str, stake: Decimal);

    async fn current(&mut self, currency_code: &str) -> TournamentInfo;

    async fn tournament_gains(&self, round_ids: Vec<String>) -> Result<Vec<TournamentUserWin>, ServerError>;
}

pub struct TournamentDemoService {}

#[async_trait]
impl TournamentService for TournamentDemoService {
    fn action(&self, _amount: Decimal, _currency_code: &str, _stake: Decimal) {}

    async fn current(&mut self, _currency_code: &str) -> TournamentInfo {
        TournamentInfo {
            current: None,
            pending_wins: false,
        }
    }

    async fn tournament_gains(&self, _round_ids: Vec<String>) -> Result<Vec<TournamentUserWin>, ServerError> {
        Ok(vec![])
    }
}

pub struct TournamentRealService {
    pub user_att_repo: Arc<UserAttributeRepository>,
    pub gain_repo: Arc<TournamentGainRepository>,
    pub tours: Arc<Vec<String>>,
    pub user: (i64, Arc<String>),
    pub server: String,
    pub client: Arc<Client>,
    pub holder: Arc<TournamentHolder>,
    pub time_range: (i64, i64),
}

impl TournamentRealService {
    const TOUR_EXCLUDE: &'static str = "exclude";
    pub async fn new(
        user_att_repo: Arc<UserAttributeRepository>,
        gain_repo: Arc<TournamentGainRepository>,
        holder: Arc<TournamentHolder>,
        user: (i64, String),
        server: String,
    ) -> Result<Self, ServerError> {
        let tours = user_att_repo.find_recursive_attrs(user.0, AttributeName::tour).await.map_err(|e| err_on!(e))?;
        let tours_on = if tours.iter().any(|t| t.value == Some(TournamentRealService::TOUR_EXCLUDE.to_string())) {
            vec![]
        } else {
            tours.into_iter().map(|t| t.value.ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))).collect::<Result<Vec<_>, _>>()?
        };
        let client = Arc::new(Client::new());

        Ok(Self {
            user_att_repo,
            gain_repo,
            tours: Arc::new(tours_on),
            user: (user.0, Arc::new(user.1)),
            holder,
            client,
            server,
            time_range: (i64::MAX, i64::MIN),
        })
    }

    async fn get_current(&mut self, currency_code: &str) -> Result<TournamentInfo, ServerError> {
        let url = Url::parse_with_params(
            &format!("{}/player/tournament", self.holder.config.url),
            &[("userName", self.user.1.as_str()), ("currency", currency_code), ("name", &self.server), ("tours", &self.tours.join(","))],
        )
        .map_err(|e| err_on!(e))?;
        let response = self.client.get(url).header(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.holder.access_token().await)).send().await.map_err(|e| err_on!(e))?;
        if response.status() != StatusCode::OK {
            if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
                self.holder.login().await;
            }
            Err(err_on!(format!("remote error tournament {}!", response.status())))
        } else {
            let t = response.json::<TournamentInfo>().await.map_err(|e| err_on!(e))?;
            if let Some(s) = t.current.as_ref() {
                self.time_range = (s.date_start.timestamp_millis(), s.date_end.timestamp_millis())
            };
            Ok(t)
        }
    }
}

#[async_trait]
impl TournamentService for TournamentRealService {
    fn action(&self, amount: Decimal, currency_code: &str, stake: Decimal) {
        if self.tours.is_empty() || amount == Decimal::ZERO {
            return;
        }
        let now = Local::now().timestamp_millis();
        if now < self.time_range.0 || now > self.time_range.1 {
            return;
        }

        let client = Arc::clone(&self.client);
        let holder = Arc::clone(&self.holder);
        let config = Arc::clone(&self.holder.config);
        let tours = self.tours.clone();
        let user_name = self.user.1.clone();
        let currency = currency_code.to_string();
        tokio::spawn(async move {
            let url = Url::parse(&format!("{}/player/act", config.url));
            match url {
                Ok(u) => {
                    let token = holder.access_token().await;
                    let answer = client
                        .post(u)
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
                        .json(&TournamentIncreaseRequest {
                            amount,
                            currency,
                            ip: config.ip.clone(),
                            stake,
                            tours,
                            user_name,
                        })
                        .send()
                        .await;
                    match answer {
                        Ok(r) => {
                            if r.status() == StatusCode::UNAUTHORIZED || r.status() == StatusCode::FORBIDDEN {
                                error!("Tournament status error: {:?}", r.status());
                                holder.login().await;
                            }
                        }
                        Err(e) => {
                            error!("Tournament request error: {e}!")
                        }
                    }
                }
                Err(e) => {
                    error!("Tournament url error: {e}")
                }
            }
        });
    }

    async fn current(&mut self, currency_code: &str) -> TournamentInfo {
        match self.get_current(currency_code).await {
            Ok(i) => i,
            Err(e) => {
                error!("error on tournament request {e}");
                TournamentInfo {
                    current: None,
                    pending_wins: false,
                }
            }
        }
    }

    async fn tournament_gains(&self, round_ids: Vec<String>) -> Result<Vec<TournamentUserWin>, ServerError> {
        self.gain_repo.find_gains_by_rounds(round_ids).await.map_err(|e| err_on!(e))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentCreateAct {
    pub outbound_id: Uuid,
    pub remote_code: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentRemoteAct {
    pub id: i64,
    pub remote_code: i32,
}

pub struct TournamentGainService {
    http: Arc<Client>,
    holder: Arc<TournamentHolder>,
}

impl TournamentGainService {
    pub fn new(holder: Arc<TournamentHolder>) -> Self {
        Self {
            http: Arc::new(Client::new()),
            holder,
        }
    }
    pub async fn commit_wins(&self, wins: Vec<TournamentCreateAct>) {
        if wins.is_empty() {
            return;
        }

        let client = Arc::clone(&self.http);
        let config = Arc::clone(&self.holder.config);
        let token = self.holder.access_token().await;
        tokio::spawn(async move {
            let url = Url::parse(&format!("{}/player/commitWins", config.url));
            match url {
                Ok(u) => {
                    let answer = client
                        .post(u)
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
                        .json(&wins)
                        .send()
                        .await;
                    match answer {
                        Ok(response) => {
                            if response.status() != StatusCode::NO_CONTENT && response.status() != StatusCode::OK {
                                error!("remote error on commit_wins {:?}", response.status());
                            }
                        }
                        Err(e) => {
                            error!("error commit_wins: {e}")
                        }
                    }
                }
                Err(e) => {
                    error!("error commit_win url - {e}!")
                }
            }
        });
    }

    pub async fn commit_win(&self, act: TournamentRemoteAct) {
        let client = Arc::clone(&self.http);
        let config = Arc::clone(&self.holder.config);
        let token = self.holder.access_token().await;
        tokio::spawn(async move {
            let url = Url::parse(&format!("{}/player/commitWin", config.url));
            match url {
                Ok(u) => {
                    let answer = client
                        .post(u)
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
                        .json(&act)
                        .send()
                        .await;
                    match answer {
                        Ok(response) => {
                            if response.status() != StatusCode::NO_CONTENT && response.status() != StatusCode::OK {
                                error!("remote error on commit_win {:?}", response.status());
                            }
                        }
                        Err(e) => {
                            error!("error commit_win: {e}")
                        }
                    }
                }
                Err(e) => {
                    error!("error commit_win url - {e}!")
                }
            }
        });
    }
}
