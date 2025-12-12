use crate::admin::StepSettings;
use crate::protocol::{CurrencyData, JackpotsWinData, JoinData, LoginData, Response, TournamentUserWin};
use crate::tournament::{
    TournamentDemoService, TournamentGainService, TournamentHolder, TournamentInfo, TournamentPlace, TournamentRealService, TournamentRemoteAct, TournamentService,
    TournamentWinData,
};
use async_trait::async_trait;
use chrono::{Duration, Local};
use essential_core::account_service::err_code::OPERATION_NOT_ALLOWED;
use essential_core::account_service::{
    acc_err, err_code, AccountBalanceRequest, AccountConfig, AccountError, AccountRequest, AccountResultRequest, AccountService, AccountServiceFactory, AccountWagerRequest,
    DeferredFactory, GameConfig, GameStatus, ProxyAlias, RequestConfig, ResultError, RetryConfig, TournamentRequest,
};
use essential_core::err_on;
use essential_core::error::message::{BALANCE_NONE_ERROR, CURRENCY_CODE_ERROR, CURRENCY_OPERATOR_ERROR};
use essential_core::error::ServerError;
use essential_core::jackpot_service::{JackpotAwardProxy, JackpotProxy};
use essential_data::currency::CurrencyShort;
use essential_data::repo::{BaseRepository, CurrencyRepository, Repository, SqlSafeAction, TCurrencyRepository, UserAttributeRepository, UserInformationRepository};
use essential_data::user_attribute::AttributeName;
use essential_data::user_user::UserShort;
use fugaso_data::repo::{GameRepository, TournamentGainRepository};
use fugaso_data::{fugaso_action, fugaso_game, fugaso_round, promo_account, promo_stats, promo_transaction, tournament_gain};
use fugaso_math::math::SlotMath;
use fugaso_math::protocol::{id, Promo};
use log::{debug, error, warn};
use moka::future::Cache;
use num_traits::cast::ToPrimitive;
use reqwest::{ClientBuilder, Url};
use sea_orm::prelude::Decimal;
use sea_orm::{Set, Unchanged};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Mul;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

pub trait JackpotProxyFactory {
    fn create_jackpot_proxy(&self, math_class: &str) -> Box<dyn JackpotProxy + Send + Sync>;
}

pub trait RetryServiceFactory {
    fn create_retry_service(&self, factory: DeferredFactory) -> Box<dyn RetryService + Send + Sync>;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthData {
    pub user_name: Option<String>,
    pub session_id: Option<String>,
    pub operator_id: Option<i64>,
    pub game_name: String,
    pub mode: ProxyAlias,
    pub password: Option<String>,
}

#[derive(Debug)]
pub struct TournamentTransfer {
    pub balance: i64,
    pub points: Decimal,
    pub place: i32,
    pub name: String,
    pub amount: Decimal,
    pub winners: Arc<Vec<TournamentPlace>>,
}

#[derive(Debug, Default, Clone)]
pub struct BetSettings {
    pub max_win: Decimal,
    pub max_stake: Decimal,
}

impl From<HashMap<&AttributeName, Decimal>> for BetSettings {
    fn from(value: HashMap<&AttributeName, Decimal>) -> Self {
        Self {
            max_win: *value.get(&AttributeName::maxWin).unwrap_or(&Decimal::ZERO),
            max_stake: *value.get(&AttributeName::maxStake).unwrap_or(&Decimal::ZERO),
        }
    }
}

pub struct GameService {
    game_repo: Arc<GameRepository>,
    cache_name: Cache<String, Option<fugaso_game::Model>>,
    cache_id: Cache<i64, Option<fugaso_game::Model>>,
}

impl GameService {
    pub fn new(game_repo: Arc<GameRepository>) -> Self {
        Self {
            game_repo,
            cache_name: Cache::builder()
                // Max 10,000 entries
                .max_capacity(500)
                // Time to live (TTL): 30 minutes
                .time_to_live(std::time::Duration::from_secs(10 * 60))
                // Create the cache.
                .build(),
            cache_id: Cache::builder()
                // Max 10,000 entries
                .max_capacity(500)
                // Time to live (TTL): 30 minutes
                .time_to_live(std::time::Duration::from_secs(10 * 60))
                // Create the cache.
                .build(),
        }
    }
    pub async fn get_game(&self, game_name: &str) -> Result<Option<fugaso_game::Model>, ServerError> {
        let repo = self.game_repo.clone();
        self.cache_name.try_get_with(game_name.to_string(), async move { repo.find_by_name(game_name).await.map_err(|e| err_on!(e)) }).await.map_err(|e| (*e).clone())
    }

    pub async fn get_game_by_id(&self, id: i64) -> Result<Option<fugaso_game::Model>, ServerError> {
        let repo = self.game_repo.clone();
        self.cache_id.try_get_with(id, async move { repo.find_by_id(id).await.map_err(|e| err_on!(e)) }).await.map_err(|e| (*e).clone())
    }
}

pub fn is_rollback_code(rc: i32) -> bool {
    rc == err_code::IO_ERROR.0 || rc == err_code::HTTP_ERROR.0 || rc == err_code::FORMAT_ERROR.0
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    pub start_amount: Decimal,
    pub currency: Option<CurrencyShort>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            start_amount: Decimal::new(3000, 0),
            currency: None,
        }
    }
}

pub struct SlotProxy<M: SlotMath, F: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory> {
    factory: Arc<F>,
    jackpot_award_proxy: Box<dyn JackpotAwardProxy + Send + Sync>,
    service: Box<dyn AccountService + Send + Sync>,
    game_service: Arc<GameService>,
    base_repo: Arc<BaseRepository>,
    currency_repo: Arc<CurrencyRepository>,
    user_repo: Arc<UserAttributeRepository>,
    gain_repo: Arc<TournamentGainRepository>,
    tour_holder: Arc<TournamentHolder>,

    game: fugaso_game::Model,
    user: UserShort,
    currency_id: i64,
    currency_code: String,
    tour_client: Box<dyn TournamentService + Send + Sync>,
    gain_service: Arc<TournamentGainService>,
    info_service: Arc<UserInformationService>,
    retry_service: Box<dyn RetryService + Send + Sync>,
    pub bet_settings: BetSettings,
    pub step_settings: StepSettings,
    pub credits: Decimal,
    pub config: ProxyConfig,
    pub phantom: PhantomData<M>,
    pub game_session_id: Option<String>,
}

impl<M: SlotMath, F: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory> SlotProxy<M, F>
where
    <M as SlotMath>::Special: Serialize + Sync + Send,
{
    pub fn balance(&self) -> i64 {
        let b = self.credits.mul(Decimal::new(100, 0));
        b.to_i64().unwrap_or(0)
    }
    pub async fn new(
        factory: Arc<F>,
        jackpot_award_proxy: Box<dyn JackpotAwardProxy + Send + Sync>,
        game_service: Arc<GameService>,
        base_repo: Arc<BaseRepository>,
        currency_repo: Arc<CurrencyRepository>,
        user_repo: Arc<UserAttributeRepository>,
        gain_repo: Arc<TournamentGainRepository>,
        tour_holder: Arc<TournamentHolder>,
        gain_service: Arc<TournamentGainService>,
        info_service: Arc<UserInformationService>,
        config: ProxyConfig,
        game: fugaso_game::Model,
        step_settings: StepSettings,
    ) -> Result<Self, ServerError> {
        let service = factory.create_account_service(&ProxyAlias::Demo).await?;
        Ok(Self {
            factory,
            jackpot_award_proxy,
            service,
            game_service,
            base_repo,
            user_repo,
            gain_repo,
            currency_repo,
            tour_holder,

            game,
            user: Default::default(),
            currency_code: "".to_string(),
            bet_settings: BetSettings::default(),
            step_settings,
            credits: Decimal::ZERO,
            currency_id: 0,
            config,
            tour_client: Box::new(TournamentDemoService {}),
            gain_service,
            info_service,
            retry_service: Box::new(DefaultRetryService),
            phantom: PhantomData,
            game_session_id: None,
        })
    }
    pub async fn login(
        &mut self,
        auth: AuthData,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        demo_user_id: Option<i64>,
    ) -> Result<(i64, Option<String>), ServerError> {
        if auth.mode != ProxyAlias::Demo {
            self.service = self.factory.create_account_service(&auth.mode).await?;
        }
        self.game = self.game_service.get_game(&auth.game_name).await?.ok_or_else(|| err_on!(format!("game {} is not found!", auth.game_name)))?;

        let currency_config = if let Some(d) = demo_user_id {
            debug!("demo user={d}");
            self.currency_repo
                .find_user_currency(d)
                .await
                .map_err(|e| err_on!(e))?
                .and_then(|c| {
                    c.code.zip(c.symbol).map(|(code, symbol)| CurrencyShort {
                        id: c.id,
                        name: code.clone(),
                        code,
                        symbol,
                    })
                })
                .or(self.config.currency.clone())
        } else {
            self.config.currency.clone()
        };

        let jackpot_proxy = self.factory.create_jackpot_proxy(&self.game.math_class);
        self.service
            .config_account(AccountConfig {
                operator_id: auth.operator_id,
                jackpot_proxy: Some(jackpot_proxy),
                start_amount: Some(self.config.start_amount),
                api_version: "1.0.0".to_string(),
                currency: currency_config,
                ..Default::default()
            })
            .await?;
        self.service.config_retry(RetryConfig {
            urgent_attempts: 6,
            ..Default::default()
        });
        let (rsp, user) = self
            .service
            .get_account(AccountRequest {
                name: auth.user_name,
                session: auth.session_id,
                password: auth.password,
                expire_period: Duration::hours(2),
                blocked: false,
                mode: auth.mode,
                ..Default::default()
            })
            .await?;
        self.game_session_id = rsp.game_session_id.clone();

        self.user = user;
        self.currency_id = self.user.currency_id.ok_or_else(|| err_on!(CURRENCY_OPERATOR_ERROR))?;
        self.currency_code = self.user.currency_code.clone().ok_or_else(|| err_on!(CURRENCY_CODE_ERROR))?;
        let (bet_settings, deferred, step_settings, request_cfg) = if self.service.is_demo() {
            (BetSettings::default(), None, None, None)
        } else {
            let mut att_names = vec![AttributeName::maxWin, AttributeName::maxStake, AttributeName::retryCfg, AttributeName::requestCfg];
            let game_att = AttributeName::from_str(&self.game.math_class).map_or_else(
                |e| {
                    warn!("wrong attribute name {e}!");
                    None
                },
                |v| Some(v),
            );
            if let Some(n) = &game_att {
                att_names.push(n.clone());
            }
            let map = self
                .user_repo
                .find_recursive_attrs_in_keys(self.user.id, att_names)
                .await
                .map_err(|e| err_on!(e))?
                .into_iter()
                .filter_map(|a| {
                    a.a_key
                        .as_ref()
                        .and_then(|k| match AttributeName::from_str(&k) {
                            Ok(a) => Some(a),
                            Err(e) => {
                                warn!("wrong attribute name {e}!");
                                None
                            }
                        })
                        .and_then(|k| a.value.map(|v| (k, v)))
                })
                .collect::<HashMap<_, _>>();
            let map_attrs = map
                .iter()
                .filter(|e| e.0 == &AttributeName::maxStake || e.0 == &AttributeName::maxWin)
                .map(|(k, v)| {
                    Decimal::from_str(&v).map_or_else(
                        |e| {
                            warn!("error parse attr {k} - {e}!");
                            (k, Decimal::ZERO)
                        },
                        |d| (k, d),
                    )
                })
                .fold(HashMap::new(), |mut acc, p| {
                    if !acc.contains_key(&p.0) {
                        acc.insert(p.0, p.1);
                    }
                    acc
                });

            let deferred = map.get(&AttributeName::retryCfg).and_then(|v| {
                DeferredFactory::from_str(&v).map_or_else(
                    |e| {
                        warn!("error parse attr {} - {e}!", AttributeName::retryCfg);
                        None
                    },
                    |v| Some(v),
                )
            });

            let step_settings = game_att.and_then(|n| map.get(&n).map(|v| (n, v))).map_or(None, |(n, v)| {
                serde_json::from_str(v).map_or_else(
                    |e| {
                        warn!("error parse attr {n} - {e}!");
                        None
                    },
                    |v| Some(v),
                )
            });

            let request_cfg: Option<RequestConfig> = map.get(&AttributeName::requestCfg).and_then(|v| {
                serde_json::from_str(v).map_or_else(
                    |e| {
                        warn!("error parse attr {} - {e}!", AttributeName::requestCfg);
                        None
                    },
                    |v| Some(v),
                )
            });

            (map_attrs.into(), deferred, step_settings, request_cfg)
        };
        if let Some(s) = step_settings {
            self.step_settings = s;
        }
        self.bet_settings = bet_settings;

        if let Some(r) = request_cfg {
            self.service.config_http(ClientBuilder::new().connect_timeout(r.connect_timeout()).timeout(r.timeout()))?;
        }
        if let Some(f) = deferred {
            self.retry_service = self.factory.create_retry_service(f);
            self.service.config_retry(RetryConfig {
                urgent_attempts: 1,
            });
        }

        self.service
            .config_game(GameConfig {
                name: self.game.game_name.clone(),
                game_id: Some(self.game.id),
                room_id: Some(self.game.id),
            })
            .await?;

        let jackpot_ids = self.service.jackpot_ids();
        self.jackpot_award_proxy.init(jackpot_ids).await?;
        if !self.is_demo() {
            let info_service = Arc::clone(&self.info_service);
            let user_id = self.user.id;
            let login_ip = self.user.login_ip.clone();
            tokio::spawn(async move { info_service.locate(user_id, ip_address_list, user_agent, login_ip).await });
            if self.game.tour_theme.as_ref().map(|v| v.len() > 0).unwrap_or(false) {
                self.tour_client = Box::new(
                    TournamentRealService::new(
                        Arc::clone(&self.user_repo),
                        Arc::clone(&self.gain_repo),
                        Arc::clone(&self.tour_holder),
                        (self.user.id, self.user.user_name.clone()),
                        self.tour_holder.ident(),
                    )
                    .await?,
                );
            }
        }
        Ok((self.user.id, rsp.game_session_id))
    }

    pub async fn get_balance(&mut self) -> Result<i64, ServerError> {
        let rsp_balance = self.service.get_balance(AccountBalanceRequest::default()).await?;
        self.credits = rsp_balance.balance.ok_or_else(|| err_on!(BALANCE_NONE_ERROR))?.into();
        Ok(self.balance())
    }

    pub async fn result(
        &mut self,
        action: &fugaso_action::Model,
        round: &fugaso_round::Model,
        status: GameStatus,
        promo: PromoValue,
        game_session_id: Option<String>,
    ) -> Result<Decimal, AccountError> {
        let common_id = round.common_id.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?;
        let promo_out = promo.out;
        let (offer_id, charge_id) = (promo.offer_id, promo.charge_id);
        let amount = Decimal::new(action.amount, 2);
        let stake = round.stake.map(|a| Decimal::new(a, 2)).ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?;
        let result = self
            .service
            .result(AccountResultRequest {
                amount: Decimal::new(action.amount, 2),
                action_id: action.id.to_string(),
                round_id: common_id.to_string(),
                offer_id,
                charge_id,
                promo_out,
                detail: Some(round.detail.alias().to_string()),
                status: Some(status.to_string()),
                game_session_id,
                ..Default::default()
            })
            .await;
        match result {
            Ok(rsp) => {
                self.tour_client.action(amount, &self.currency_code, stake);
                self.credits = rsp.balance.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?.into();
                Ok(self.credits)
            }
            Err(e) => {
                let err = match e {
                    ResultError::Halt {
                        err,
                    } => err,
                    ResultError::Retry {
                        url,
                        err,
                    } => {
                        self.retry_service.result(url, action.id, round.id);
                        err
                    }
                };
                if err.rc == OPERATION_NOT_ALLOWED.0 {
                    let rsp = self.service.get_balance(AccountBalanceRequest::default()).await?;
                    self.credits = rsp.balance.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?.into();
                    Ok(self.credits)
                } else {
                    Err(err)
                }
            }
        }
    }

    pub async fn wager(&mut self, action: &fugaso_action::Model, round: &fugaso_round::Model, promo: &PromoValue) -> Result<(Decimal, Decimal), AccountError> {
        let common_id = round.common_id.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?;
        let promo_out = promo.out;
        let amount = Decimal::new(action.amount, 2);
        let (offer_id, charge_id) = (promo.offer_id.clone(), promo.charge_id.clone());
        let result = self
            .service
            .wager(AccountWagerRequest {
                amount,
                action_id: action.id.to_string(),
                round_id: common_id.to_string(),
                offer_id,
                charge_id: charge_id.clone(),
                promo_out,
                detail: Some(round.detail.alias().to_string()),
                ..Default::default()
            })
            .await;
        match result {
            Ok(rsp) => {
                self.tour_client.action(amount, &self.currency_code, amount);
                self.credits = rsp.balance.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?.into();
                Ok((self.credits, amount))
            }
            Err(e) => {
                /*if e.rc == OPERATION_NOT_ALLOWED.0 {
                    let rsp = self
                        .service
                        .get_balance(AccountBalanceRequest::default())
                        .await?;
                    self.credits = rsp
                        .balance
                        .ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?
                        .into();
                    Ok((amount, self.credits))
                } else*/
                if is_rollback_code(e.rc) {
                    self.rollback(
                        AccountWagerRequest {
                            amount,
                            action_id: action.id.to_string(),
                            round_id: common_id.to_string(),
                            offer_id,
                            charge_id,
                            promo_out,
                            detail: Some(round.detail.alias().to_string()),
                            ..Default::default()
                        },
                        action.id,
                        round.id,
                    )
                    .await;
                    Err(e.into())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn rollback(&mut self, request: AccountWagerRequest, action_id: i64, round_id: i64) {
        if let Err(e) = self.service.rollback(request).await {
            match e {
                ResultError::Halt {
                    ..
                } => {}
                ResultError::Retry {
                    url,
                    ..
                } => self.retry_service.rollback(url, action_id, round_id),
            }
        }
    }

    pub async fn tournament_win(&mut self, win: TournamentWinData, round_id: Option<i64>) -> Option<TournamentTransfer> {
        match self.pay_tournament_win(win, round_id).await {
            Ok(w) => w,
            Err(e) => {
                error!("error tournament win {e:?}!");
                None
            }
        }
    }

    async fn pay_tournament_win(&mut self, win: TournamentWinData, common_id: Option<i64>) -> Result<Option<TournamentTransfer>, ServerError> {
        let gain = win.gain;
        let round_id = common_id.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?;
        let gain_on = self
            .base_repo
            .store_safe(SqlSafeAction::Update(tournament_gain::ActiveModel {
                id: Unchanged(gain.id),
                round_id: Set(round_id.to_string()),
                remote_code: Set(err_code::GENERAL_CODE),
                opt_lock: Set(gain.opt_lock),
                ..Default::default()
            }))
            .await
            .map_err(|e| err_on!(e))?;
        match self
            .service
            .tournament(TournamentRequest {
                amount: gain_on.amount,
                name: gain_on.tour,
                outbound_id: gain_on.inbound_id.to_string(),
                round_id: gain_on.round_id.to_string(),
            })
            .await
        {
            Ok(rsp) => {
                self.credits = rsp.balance.ok_or_else(|| acc_err(err_code::TECHNICAL_ERROR, line!(), file!()))?.into();
                let gain_done = self
                    .base_repo
                    .store_safe(SqlSafeAction::Update(tournament_gain::ActiveModel {
                        id: Unchanged(gain_on.id),
                        time_done: Set(Local::now().naive_local()),
                        remote_code: Set(rsp.rc),
                        remote_id: Set(rsp.account_tran_id),
                        opt_lock: Set(gain_on.opt_lock),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|e| err_on!(e))?;
                self.gain_service
                    .commit_win(TournamentRemoteAct {
                        id: win.award_id,
                        remote_code: gain_done.remote_code,
                    })
                    .await;
                Ok(Some(TournamentTransfer {
                    balance: self.balance(),
                    points: win.balance,
                    place: gain_done.place,
                    name: gain_done.tour,
                    amount: gain_done.amount,
                    winners: win.winners,
                }))
            }
            Err(e) => {
                let gain_done = self
                    .base_repo
                    .store_safe(SqlSafeAction::Update(tournament_gain::ActiveModel {
                        id: Unchanged(gain_on.id),
                        remote_code: Set(e.rc),
                        remote_message: Set(Some(e.message)),
                        opt_lock: Set(gain_on.opt_lock),
                        ..Default::default()
                    }))
                    .await
                    .map_err(|e| err_on!(e))?;
                self.gain_service
                    .commit_win(TournamentRemoteAct {
                        id: win.award_id,
                        remote_code: gain_done.remote_code,
                    })
                    .await;
                Ok(None)
            }
        }
    }

    pub fn user(&self) -> &UserShort {
        &self.user
    }

    pub fn game(&self) -> &fugaso_game::Model {
        &self.game
    }

    pub fn is_demo(&self) -> bool {
        self.service.is_demo()
    }

    pub fn currency(&self) -> Result<(i64, String), ServerError> {
        Ok((self.currency_id, self.currency_code.clone()))
    }

    pub async fn join(&self) -> Result<Vec<Response<M::Special, M::Restore>>, ServerError> {
        let currencies = self.currency_repo.find_all().await.map_err(|e| err_on!(e))?;
        let mut packets = vec![Response::Login(LoginData {
            id: id::LOGIN,
            game_id: self.game.id,
            game_name: self.game.game_name.as_ref().map(|g| g.to_string()).unwrap_or(String::from("-")),
        })];
        packets.append(
            &mut currencies
                .into_iter()
                .map(|c| {
                    Response::Currency(CurrencyData {
                        id: 0,
                        code: c.code,
                        symbol: c.symbol,
                    })
                })
                .collect::<Vec<_>>(),
        );
        packets.push(Response::Join(JoinData {
            id: id::JOIN,
            user_id: self.user.id,
            nickname: self.user.user_name.clone(),
            currency: self.user.currency_code.clone(),
        }));

        Ok(packets)
    }

    pub async fn check_jackpots(&mut self, stake: Decimal, round_id: i64) -> Result<(Response<M::Special, M::Restore>, usize), ServerError> {
        let requests = self.jackpot_award_proxy.check(self.currency_id, stake, round_id).await?;
        let mut jackpots = HashMap::new();
        for j in requests {
            let result = self.service.jackpot(j).await;
            match result {
                Ok((r, p)) => {
                    self.credits = r.balance.ok_or_else(|| err_on!(BALANCE_NONE_ERROR))?.into();
                    jackpots.insert(p.name, p.amount);
                }
                Err(_) => {}
            }
        }
        let len = jackpots.len();
        Ok((
            Response::JackpotsWin(JackpotsWinData {
                jackpots,
                collected: None,
                balance: 0,
            }),
            len,
        ))
    }

    pub fn set_balance(&mut self, balance: Decimal) -> Result<(), ServerError> {
        let rsp = self.service.set_balances(Decimal::ZERO, balance)?;
        self.credits = rsp.balance.ok_or_else(|| err_on!(BALANCE_NONE_ERROR))?.into();
        Ok(())
    }

    pub async fn tournament(&mut self) -> TournamentInfo {
        self.tour_client.current(&self.currency_code).await
    }

    pub async fn tournament_gains(&self, round_ids: Vec<String>) -> Result<Vec<TournamentUserWin>, ServerError> {
        self.tour_client.tournament_gains(round_ids).await
    }

    pub async fn close(&self) {
        self.service.flush().await;
        if self.game_session_id.is_some() {
            if let Err(_) = self.service.close().await {}
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PromoValue {
    pub out: i64,
    pub offer_id: Option<Uuid>,
    pub charge_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct PromoInfo {
    pub out: i64,
    pub stake: i64,
    pub amount: i32,
    pub multiplier: i32,
    pub offer_id: Option<Uuid>,
    pub charge_id: Option<String>,
}

impl From<PromoInfo> for PromoValue {
    fn from(value: PromoInfo) -> Self {
        Self {
            out: value.out,
            offer_id: value.offer_id,
            charge_id: value.charge_id,
        }
    }
}

#[async_trait]
pub trait PromoService {
    async fn activate(&mut self, fresh: bool) -> Result<PromoInfo, ServerError>;
    async fn decrement(
        &self,
        _round_id: i64,
        _external_id: Option<String>,
        _total_bet: i64,
    ) -> Result<(Option<promo_transaction::Model>, Option<promo_account::ActiveModel>, Option<promo_stats::ActiveModel>, Option<PromoInfo>, Promo), ServerError> {
        Ok((
            None,
            None,
            None,
            None,
            Promo {
                amount: 0,
                multi: 0,
            },
        ))
    }

    async fn find_promo(&self, _round_id: i64) -> Result<(PromoValue, Promo), ServerError> {
        Ok((PromoValue::default(), Promo::default()))
    }

    fn commit(&mut self, account: Option<promo_account::Model>, stats: Option<promo_stats::Model>);

    fn promo_state(&self) -> Promo {
        Promo {
            amount: 0,
            multi: 0,
        }
    }

    fn increment(&self, _amount: i64) -> Option<promo_stats::ActiveModel> {
        None
    }
}

pub struct DemoPromoService;

#[async_trait]
impl PromoService for DemoPromoService {
    async fn activate(&mut self, _fresh: bool) -> Result<PromoInfo, ServerError> {
        Ok(PromoInfo::default())
    }
    fn commit(&mut self, _account: Option<promo_account::Model>, _stats: Option<promo_stats::Model>) {}
}

#[async_trait]
pub trait PromoServiceFactory {
    async fn create_real_promo_service(&self, user_id: i64, game_id: i64) -> Result<Box<dyn PromoService + Send + Sync>, ServerError>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpResponse {
    pub country_code: String,
    pub city: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct IpServiceConfig {
    pub url: String,
    pub key: String,
}

pub struct UserInformationService {}

impl UserInformationService {
    #[allow(unused_variables)]
    pub fn new(base_repo: Arc<BaseRepository>, user_info_repo: Arc<UserInformationRepository>, config: IpServiceConfig) -> Self {
        Self {}
    }

    #[allow(unused_variables)]
    pub async fn locate(&self, user_id: i64, ip_address_list: Option<String>, user_agent: Option<String>, login_ip: Option<String>) {
        match self.do_locate(user_id, ip_address_list, user_agent, login_ip).await {
            Ok(_) => {}
            Err(e) => error!("error locate user: {e}!"),
        }
    }

    #[allow(unused_variables)]
    async fn do_locate(&self, user_id: i64, ip_address_list: Option<String>, user_agent: Option<String>, login_ip: Option<String>) -> Result<(), ServerError> {
        Ok(())
    }
}

pub trait RetryService {
    #[allow(unused_variables)]
    fn result(&self, url: Url, action_id: i64, round_id: i64) {}

    #[allow(unused_variables)]
    fn rollback(&self, url: Url, action_id: i64, round_id: i64) {}
}

pub struct DefaultRetryService;

impl RetryService for DefaultRetryService {}
