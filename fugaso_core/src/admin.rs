use crate::protocol::{HistoryRequest, Response, RoundStory, TournamentUserWin};
use crate::proxy::{BetSettings, DemoPromoService, GameService, PromoInfo, PromoService, PromoServiceFactory, PromoValue};
use chrono::Local;
use essential_core::account_service::{err_code, AccountError, GameStatus};
use essential_core::err_on;
use essential_core::error::message::{exchange_msg, EURO_CURRENCY_ERROR, GAME_BET_ERROR, ILLEGAL_ARGUMENT};
use essential_core::error::ServerError;
use essential_data::repo::SqlAction::{Insert, Update};
use essential_data::repo::{
    BaseRepository, CurrencyRepository, DemoRepository, ExchangeRateRepository, Repository, SqlSafeAction, TCurrencyRepository, TypedRepository, UserSettingsRepository,
};
use essential_data::{currency, exchange_rate, user_settings};
use fugaso_data::fugaso_action::ActionKind;
use fugaso_data::fugaso_round::{RoundDetail, RoundStatus};
use fugaso_data::model::ActiveClone;
use fugaso_data::repo::{PercentRepository, RoundRepository};
use fugaso_data::sequence_generator::{FugasoIdGenerator, IdGenerator, IdGeneratorFactory};
use fugaso_data::{common_round, fugaso_action, fugaso_game, fugaso_percent, fugaso_round, promo_account, promo_stats, promo_transaction};
use fugaso_data::{common_round::Model as CommonRound, fugaso_action::Model as Action, fugaso_round::Model as Round};
use fugaso_math::fsm::FSM;
use fugaso_math::math::{self, BetCalculator, GameInitArg, GamePlayInput, IRequest, JoinArg, MathSettings, ProxyMath, ReplayMath, SlotMath, SpinArg, Step};
use fugaso_math::protocol::{GameData, GameResult, Promo, SpinData};
use fugaso_math::validator::{SimpleValidator, Validator};
use log::{debug, error, warn};
use num_traits::ToPrimitive;
use sea_orm::prelude::async_trait::async_trait;
use sea_orm::prelude::Decimal;
use sea_orm::{IntoActiveModel, Set, TryIntoModel, Unchanged};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait StateLoader {
    async fn find_last_round(&self, user_id: i64, game_id: i64) -> Result<Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>, ServerError>;
}

pub struct SuccessStateLoader {
    pub round_repo: Arc<RoundRepository>,
}

#[async_trait]
impl StateLoader for SuccessStateLoader {
    async fn find_last_round(&self, user_id: i64, game_id: i64) -> Result<Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>, ServerError> {
        self.round_repo.find_last_round(user_id, game_id, RoundStatus::SUCCESS).await.map_err(|e| err_on!(e))
    }
}

#[async_trait]
pub trait TypedRepoFactory<
    A = common_round::ActiveModel,
    B = fugaso_round::ActiveModel,
    C = fugaso_action::ActiveModel,
    D = promo_transaction::ActiveModel,
    E = promo_account::ActiveModel,
    F = promo_stats::ActiveModel,
>
{
    async fn create_repo(&self, user_id: i64) -> Result<Arc<dyn TypedRepository<A, B, C, D, E, F> + Send + Sync>, ServerError>;
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminConfig {
    pub history_limit: u64,
}

pub struct SlotAdmin<M: SlotMath, F: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory, S: StateLoader> {
    factory: Arc<F>,
    type_repo: Arc<
        dyn TypedRepository<
                common_round::ActiveModel,
                fugaso_round::ActiveModel,
                fugaso_action::ActiveModel,
                promo_transaction::ActiveModel,
                promo_account::ActiveModel,
                promo_stats::ActiveModel,
            > + Send
            + Sync,
    >,
    base_repo: Arc<BaseRepository>,
    percent_repo: Arc<PercentRepository>,
    user_settings_repo: Arc<UserSettingsRepository>,
    table_id_gen: Arc<dyn IdGenerator + Send + Sync>,
    round_repo: Arc<RoundRepository>,
    config: Arc<AdminConfig>,
    state_loader: S,

    user_id: i64,
    game: (i64, Option<String>),
    combo_gen: Box<dyn ComboService + Send + Sync>,
    pub math: ProxyMath<M>,
    bet_calculator: M::Calculator,
    promo_service: Box<dyn PromoService + Send + Sync>,
    fsm: M::PlayFSM,
    configurator: Arc<BetConfigurator>,
    step: Step,
    validator: M::V,
    input: RoundInput<M::Input>,
    round: fugaso_round::Model,
    result: Arc<GameData<M::Special, M::Restore>>,
}

#[derive(Debug)]
pub struct RoundInput<R: IRequest + Default> {
    pub request: R,
    pub promo_value: PromoValue,
}

impl<R: IRequest + Default> Default for RoundInput<R> {
    fn default() -> Self {
        Self {
            request: R::default(),
            promo_value: PromoValue::default(),
        }
    }
}

pub struct InitArg {
    pub user_id: i64,
    pub game: fugaso_game::Model,
    pub demo: bool,
    pub country: Option<String>,
    pub bet_settings: BetSettings,
    pub step_settings: StepSettings,
    pub currency: (i64, String),
    pub round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
}

#[async_trait]
pub trait ComboService {
    async fn find_combo(&self, user_id: i64) -> Option<Vec<usize>>;
}

pub struct DemoComboService;

#[async_trait]
impl ComboService for DemoComboService {
    async fn find_combo(&self, _user_id: i64) -> Option<Vec<usize>> {
        None
    }
}

pub struct RealComboService {
    pub base_repo: Arc<BaseRepository>,
    pub user_settings_repo: Arc<UserSettingsRepository>,
}

#[async_trait]
impl ComboService for RealComboService {
    async fn find_combo(&self, user_id: i64) -> Option<Vec<usize>> {
        match self.user_settings_repo.find_combo(user_id).await {
            Ok(Some((id, stops))) => {
                match self
                    .base_repo
                    .store(Update(user_settings::ActiveModel {
                        id: Unchanged(id),
                        user_combo: Set(None),
                        ..Default::default()
                    }))
                    .await
                {
                    Ok(_) => Some(stops),
                    Err(e) => {
                        error!("{e}");
                        None
                    }
                }
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }
}

impl<M: SlotMath, F: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory, S: StateLoader> SlotAdmin<M, F, S>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: 'static,
{
    pub fn new(
        factory: Arc<F>,
        base_repo: Arc<BaseRepository>,
        percent_repo: Arc<PercentRepository>,
        user_settings_repo: Arc<UserSettingsRepository>,
        table_id_gen: Arc<FugasoIdGenerator>,
        round_repo: Arc<RoundRepository>,
        configurator: Arc<BetConfigurator>,
        config: Arc<AdminConfig>,
        math: M,
        state_loader: S,
    ) -> Self {
        let bet_calculator = math.create_bet_calculator();
        let fsm = math.create_fsm("");
        Self {
            factory,
            type_repo: Arc::new(DemoRepository {}),
            base_repo,
            table_id_gen,
            percent_repo,
            user_settings_repo,
            round_repo,
            config,
            user_id: 0,
            game: (0, None),
            combo_gen: Box::new(DemoComboService),
            math: ProxyMath::new(math),
            bet_calculator,
            promo_service: Box::new(DemoPromoService),
            configurator,
            fsm,
            step: Step::default(),
            validator: M::V::default(),
            input: RoundInput::default(),
            round: Round {
                multi: 1,
                common_id: Some(0),
                ..Default::default()
            },
            result: Arc::new(GameData::Spin(SpinData {
                result: GameResult {
                    ..Default::default()
                },
                next_act: ActionKind::BET,
                ..Default::default()
            })),
            state_loader,
        }
    }

    pub async fn init(&mut self, arg: InitArg) -> Result<(), ServerError> {
        self.user_id = arg.user_id;
        self.fsm = self.math.create_fsm(&arg.game.game_name.as_ref().unwrap_or(&"".to_string()));
        self.bet_calculator = self.math.create_bet_calculator();
        self.game = (arg.game.id, arg.game.game_name.clone());

        let math_settings = self.math.settings();
        let default_percent = self.get_default_percent(&math_settings, &arg.game, &arg.currency, arg.country.clone(), &arg.bet_settings).await?;
        self.step = arg.step_settings.convert(default_percent.2);
        debug!("step: {:?}", self.step);
        let round: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>;
        let mut percent: fugaso_percent::Model;
        let mut index_default: usize;
        let mut line_default: Option<usize>;
        if arg.demo {
            round = arg.round_actions;
            percent = default_percent.0;
            index_default = default_percent.1;
            line_default = default_percent.3;
            self.type_repo = Arc::new(DemoRepository {});
            self.table_id_gen = self.factory.create(arg.demo);
        } else {
            self.combo_gen = Box::new(RealComboService {
                user_settings_repo: Arc::clone(&self.user_settings_repo),
                base_repo: Arc::clone(&self.base_repo),
            });
            let percent_on = self.percent_repo.find_recursive_percent(arg.user_id, arg.game.id).await.map_err(|e| err_on!(e.to_string()))?;
            index_default = default_percent.1;
            line_default = default_percent.3;
            if let Some(p) = percent_on {
                percent = p;
            } else {
                percent = default_percent.0.clone();
            }
            if percent.poss_bets.is_none() || percent.denomination.is_none() {
                percent.poss_bets = default_percent.0.poss_bets;
                percent.denomination = default_percent.0.denomination;
            } else if percent.id != default_percent.0.id {
                let converted = self.convert_percent(&math_settings, &arg.game, &percent, &arg.currency, &arg.bet_settings).await?;
                percent.poss_bets = converted.0.poss_bets;
                percent.denomination = converted.0.denomination;
                index_default = converted.1;
                line_default = converted.2;
            }
            round = self.state_loader.find_last_round(arg.user_id, arg.game.id).await?;
        }
        self.validator = M::V::new(&percent, math_settings)?;
        self.input = RoundInput {
            request: self.validator.get_default_request(index_default, line_default),
            ..Default::default()
        };
        if let Some(r) = round {
            if r.1.len() > 0 {
                let common_id = r.0.common_id.ok_or_else(|| err_on!("common id is none!"))?;
                let (promo, promo_on) = self.promo_service.find_promo(common_id).await?;
                self.restore(r, promo, promo_on, arg.demo).await?;
            }
        }
        let promo_info = self.promo_service.activate(self.fsm.current() == ActionKind::BET).await?;
        if promo_info.amount > 0 {
            self.load_promo_input(promo_info)?;
        }
        Ok(())
    }

    pub async fn join(&self, balance: i64) -> Result<Response<M::Special, M::Restore>, ServerError> {
        let promo = self.promo_service.promo_state();
        let game_data = self.math.join(JoinArg {
            balance,
            round_id: self.round.common_id.ok_or_else(|| err_on!("common id id none!"))?,
            round_type: self.round.detail.clone(),
            round_multiplier: self.round.multi,
            curr_lines: self.input.request.line(),
            curr_bet: self.input.request.bet(),
            curr_denom: self.input.request.denom(),
            curr_reels: self.input.request.reels(),
            bet_counter: self.input.request.bet_counter(),
            next_act: self.fsm.current(),
            poss_lines: self.validator.lines(),
            poss_reels: self.validator.reels(),
            poss_bets: self.validator.bets(),
            poss_denom: self.validator.denomination(),
            poss_bet_counters: self.validator.bet_counters(),
            promo,
        })?;
        Ok(Response::GameData(Arc::new(game_data)))
    }

    pub async fn restore(&mut self, r: (fugaso_round::Model, Vec<fugaso_action::Model>), promo: PromoValue, promo_on: Promo, demo: bool) -> Result<(), ServerError> {
        self.input = RoundInput {
            request: self.validator.from_round(&r.0, self.input.request.reels()),
            promo_value: promo,
        };
        self.round = r.0;
        if self.round.timestamp_close.is_none() && self.round.status == Some(RoundStatus::SUCCESS) {
            self.validator.correct(&mut self.input.request);
        }
        self.math.init(
            GameInitArg {
                curr_lines: self.input.request.line(),
                curr_bet: self.input.request.bet(),
                curr_denom: self.input.request.denom(),
                curr_reels: self.input.request.reels(),
                round_id: self.round.common_id.ok_or_else(|| err_on!("common id is none!"))?,
                round_type: self.round.detail.clone(),
                round_multiplier: self.round.multi,
                bet_counter: self.round.bet_counter as usize,
                promo: promo_on,
            },
            &r.1,
        )?;
        if let Some(a) = r.1.iter().max_by(|a, b| a.id.cmp(&b.id)) {
            if let Some(k) = &a.next_act {
                match k {
                    ActionKind::RESPIN | ActionKind::DROP | ActionKind::FREE_SPIN => self.fsm.init(k.clone()),
                    _ => {}
                };
            }
        }

        if !demo && self.round.status == Some(RoundStatus::REMOTE_ERROR) {
            let actions =
                r.1.iter()
                    .filter(|a| {
                        if let Some(c) = a.remote_code {
                            c != err_code::GENERAL_CODE
                        } else {
                            false
                        }
                    })
                    .map(|a| fugaso_action::ActiveModel {
                        id: Unchanged(a.id),
                        remote_code: Set(Some(err_code::GENERAL_CODE)),
                        error_info: Set(None),
                        ..fugaso_action::Model::unchanged_active_model()
                    })
                    .collect();
            let round_mod = fugaso_round::ActiveModel {
                id: Unchanged(self.round.id),
                status: Set(Some(RoundStatus::SUCCESS)),
                ..fugaso_round::Model::unchanged_active_model()
            };
            let (r_on, _) = self.round_repo.update(round_mod, actions).await.map_err(|e| err_on!(e))?;
            self.round = r_on;
        }
        Ok(())
    }

    async fn convert_percent(
        &self,
        settings: &MathSettings,
        game: &fugaso_game::Model,
        percent: &fugaso_percent::Model,
        currency: &(i64, String),
        bet_settings: &BetSettings,
    ) -> Result<(fugaso_percent::Model, usize, Option<usize>), ServerError> {
        let request_default = self.configurator.get_default_settings(&game.math_class, currency, None)?;
        let source = RequestSettings::from_db(&percent.poss_bets, &percent.denomination, request_default.index_default, request_default.line_default)?;
        let settings = GameSettings::new(self.math.create_bet_calculator(), settings.lines.clone(), game.math_class.clone(), game.exposure, bet_settings)?;
        let request_settings = self.configurator.filter(settings, &source, currency).await?;
        Ok((
            fugaso_percent::Model {
                free_percent: 100,
                percent: 100,
                poss_bets: Some(request_settings.bets_str()),
                denomination: Some(request_settings.denom_str()),
                bet_multiplier: 10_000,
                ..Default::default()
            },
            request_settings.index_default,
            request_settings.line_default,
        ))
    }

    async fn get_default_percent(
        &self,
        settings: &MathSettings,
        game: &fugaso_game::Model,
        currency: &(i64, String),
        country: Option<String>,
        bet_settings: &BetSettings,
    ) -> Result<(fugaso_percent::Model, usize, Decimal, Option<usize>), ServerError> {
        let game_settings = GameSettings::new(self.math.create_bet_calculator(), settings.lines.clone(), game.math_class.clone(), game.exposure, bet_settings)?;

        let request_on = self.configurator.find_bets(game_settings, currency, country).await?;
        Ok((
            fugaso_percent::Model {
                free_percent: 100,
                percent: 100,
                poss_bets: Some(request_on.bets_str()),
                denomination: Some(request_on.denom_str()),
                bet_multiplier: 10_000,
                ..Default::default()
            },
            request_on.index_default,
            request_on.rate,
            request_on.line_default,
        ))
    }

    fn load_promo_input(&mut self, state: PromoInfo) -> Result<(), ServerError> {
        let inputs = self.bet_calculator.calc_inputs(GamePlayInput {
            line: self.validator.max_line(),
            bet: self.validator.bets(),
            denomination: self.validator.denomination(),
            side_bet: self.validator.min_bet_counter(),
        });
        let request = self.validator.get_promo_request(state.stake, inputs)?;
        self.input = RoundInput {
            request,
            promo_value: state.into(),
        };
        Ok(())
    }

    pub async fn spin(&mut self, balance: i64, mut request: M::Input) -> Result<(Response<M::Special, M::Restore>, Round, Action, PromoValue), ServerError> {
        self.fsm.client_act(ActionKind::BET)?;
        self.fsm.client_act(ActionKind::SPIN)?;
        self.validator.correct(&mut request);

        self.input = RoundInput {
            request,
            ..Default::default()
        };

        let stake = self.bet_calculator.calc_total_bet(&self.input.request);
        let now = Local::now();
        let common_id = self.table_id_gen.gen_common_round().await.map_err(|e| err_on!(e))?;
        let common_round = CommonRound {
            id: common_id,
        };
        let mut round = Round {
            id: self.table_id_gen.gen_round().await.map_err(|e| err_on!(e))?,
            game_id: Some(self.game.0),
            user_id: Some(self.user_id),
            timestamp_open: Some(now.naive_local()),
            bet: self.input.request.bet(),
            line: self.input.request.line() as i32,
            denom: self.input.request.denom(),
            reels: Some(self.input.request.reels() as i32),
            multi: 1,
            bet_counter: self.input.request.bet_counter() as i32,
            stake: Some(stake),
            win: Some(0),
            common_id: Some(common_round.id),
            status: Some(RoundStatus::SUCCESS),
            ..Default::default()
        };
        let external_id = Some(Uuid::new_v4().to_string());
        let mut amount = stake;

        let promo_change = self.promo_service.decrement(common_id, external_id.clone(), amount).await?;
        if let Some(t) = promo_change.3 {
            self.load_promo_input(t)?;
            amount = 0;
            round.bet = self.input.request.bet();
            round.line = self.input.request.line() as i32;
            round.denom = self.input.request.denom();
            round.reels = Some(self.input.request.reels() as i32);
            round.multi = promo_change.4.multi;
            round.bet_counter = self.input.request.bet_counter() as i32;
            round.detail = RoundDetail::RICH;
            round.stake = Some(amount);
        }

        let combo = self.combo_gen.find_combo(self.user_id).await;
        let result = self.math.spin(
            &self.input.request,
            SpinArg {
                balance,
                round_id: common_id,
                round_type: round.detail.clone(),
                round_multiplier: round.multi,
                next_act: self.fsm.current(),
                promo: promo_change.4,
                stake: amount,
            },
            &self.step,
            combo,
        )?;

        let free_left = result.free().map(|f| f.left).unwrap_or(0);
        if result.has_bonus() {
            self.fsm.server_act(ActionKind::BONUS_START)?;
        } else if result.has_respin() {
            self.fsm.server_act(ActionKind::RESPIN_START)?;
        } else if result.has_drop() {
            if free_left > 0 {
                self.fsm.server_act(ActionKind::FREESPIN_START)?;
            } else {
                self.fsm.server_act(ActionKind::DROP_START)?;
            }
        } else if free_left > 0 {
            self.fsm.server_act(ActionKind::FREESPIN_START)?;
        } else if result.total() > 0 {
            if result.is_gamble_end(stake) {
                self.fsm.server_act(ActionKind::GAMBLE_END)?;
            } else {
                self.fsm.server_act(ActionKind::COLLECT_START)?;
            }
        }
        let action = Action {
            id: self.table_id_gen.gen_action().await.map_err(|e| err_on!(e))?,
            amount,
            act_descr: Some(ActionKind::BET),
            round_id: Some(round.id),
            time_done: Some(now.naive_local()),
            next_act: Some(self.fsm.current()),
            external_id,
            remote_code: Some(err_code::GENERAL_CODE),
            ..result.create_action_default()?
        };

        self.result = self.math.post_process(self.fsm.current(), result)?;
        let (_, promo_acc, promo_stats, _, r, a) = self
            .type_repo
            .store_abc_def(
                promo_change.0.map(|e| Insert(e.into_active_model())),
                promo_change.1.map(|e| SqlSafeAction::Update(e)),
                promo_change.2.map(|e| SqlSafeAction::Update(e)),
                Insert(common_round.into_active_model()),
                Insert(round.into_active_model()),
                Insert(action.into_active_model()),
            )
            .await
            .map_err(|e| err_on!(e))?;
        self.promo_service.commit(promo_acc, promo_stats);
        self.round = r;
        Ok((Response::GameData(self.result.clone()), self.round.clone(), a, self.input.promo_value.clone()))
    }

    pub async fn respin(&mut self, balance: i64) -> Result<(Response<M::Special, M::Restore>, Round, Action, PromoValue), ServerError> {
        self.fsm.client_act(ActionKind::RESPIN)?;
        let combo = self.combo_gen.find_combo(self.user_id).await;
        let stake = self.bet_calculator.calc_total_bet(&self.input.request);
        let promo = self.promo_service.promo_state();
        let result = self.math.respin(
            &self.input.request,
            SpinArg {
                balance,
                round_id: self.round.common_id.ok_or_else(|| err_on!("common id is none!"))?,
                round_type: self.round.detail.clone(),
                round_multiplier: self.round.multi,
                next_act: self.fsm.current(),
                promo,
                stake,
            },
            &self.step,
            combo,
        )?;

        let left = result.free().map(|f| f.left).unwrap_or(0);
        if result.has_respin() {
            self.fsm.server_act(ActionKind::RESPIN_START)?;
        } else if left > 0 {
            self.fsm.server_act(ActionKind::FREESPIN_START)?;
        } else if result.total() > 0 {
            if result.is_gamble_end(stake) {
                self.fsm.server_act(ActionKind::GAMBLE_END)?;
            } else {
                self.fsm.server_act(ActionKind::COLLECT_START)?;
            }
        }
        self.result = self.math.post_process(self.fsm.current(), result)?;

        let now = Local::now();
        let action = Action {
            id: self.table_id_gen.gen_action().await.map_err(|e| err_on!(e))?,
            amount: self.result.total(),
            act_descr: Some(ActionKind::RESPIN),
            round_id: Some(self.round.id),
            time_done: Some(now.naive_local()),
            next_act: Some(self.fsm.current()),
            external_id: Some(Uuid::new_v4().to_string()),
            remote_code: Some(err_code::GENERAL_CODE),
            ..self.result.create_action_default()?
        };
        let a = self.type_repo.store_c(Insert(action.into_active_model())).await.map_err(|e| err_on!(e))?;
        Ok((Response::GameData(self.result.clone()), self.round.clone(), a, self.input.promo_value.clone()))
    }

    pub async fn free_spin(&mut self, balance: i64) -> Result<(Response<M::Special, M::Restore>, Round, Action, PromoValue), ServerError> {
        self.fsm.client_act(ActionKind::FREE_SPIN)?;
        let combo = self.combo_gen.find_combo(self.user_id).await;
        let stake = self.bet_calculator.calc_total_bet(&self.input.request);
        let promo = self.promo_service.promo_state();
        let result = self.math.free_spin(
            &self.input.request,
            SpinArg {
                balance,
                round_id: self.round.common_id.ok_or_else(|| err_on!("common id is none!"))?,
                round_type: self.round.detail.clone(),
                round_multiplier: self.round.multi,
                next_act: self.fsm.current(),
                promo,
                stake,
            },
            &self.step,
            combo,
        )?;

        let left = result.free().map(|f| f.left).unwrap_or(0);
        if result.has_respin() {
            self.fsm.server_act(ActionKind::RESPIN_START)?;
        } else if left > 0 {
            self.fsm.server_act(ActionKind::FREESPIN_START)?;
        } else if result.total() > 0 {
            if result.is_gamble_end(stake) {
                self.fsm.server_act(ActionKind::GAMBLE_END)?;
            } else {
                self.fsm.server_act(ActionKind::COLLECT_START)?;
            }
        }
        self.result = self.math.post_process(self.fsm.current(), result)?;

        let now = Local::now();
        let action = Action {
            id: self.table_id_gen.gen_action().await.map_err(|e| err_on!(e))?,
            amount: self.result.total(),
            act_descr: Some(ActionKind::FREE_SPIN),
            round_id: Some(self.round.id),
            time_done: Some(now.naive_local()),
            next_act: Some(self.fsm.current()),
            external_id: Some(Uuid::new_v4().to_string()),
            remote_code: Some(err_code::GENERAL_CODE),
            ..self.result.create_action_default()?
        };
        let a = self.type_repo.store_c(Insert(action.into_active_model())).await.map_err(|e| err_on!(e))?;
        Ok((Response::GameData(self.result.clone()), self.round.clone(), a, self.input.promo_value.clone()))
    }

    pub async fn collect(&mut self, balance: i64) -> Result<(Response<M::Special, M::Restore>, Round, Action, GameStatus, PromoValue), ServerError> {
        self.fsm.server_act(ActionKind::COLLECT)?;
        let now = Local::now();
        let left = self.result.free().map(|f| f.left).unwrap_or(0);
        let time_close = if left > 0 {
            self.fsm.server_act(ActionKind::FREESPIN_START)?;
            None
        } else if self.result.has_drop() {
            self.fsm.server_act(ActionKind::DROP_START)?;
            None
        } else {
            Some(Local::now().naive_local())
        };
        let promo_on = if self.round.detail == RoundDetail::RICH {
            self.promo_service.increment(self.result.total())
        } else {
            None
        };
        let action = Action {
            id: self.table_id_gen.gen_action().await.map_err(|e| err_on!(e))?,
            amount: self.result.total(),
            act_descr: Some(ActionKind::COLLECT),
            round_id: Some(self.round.id),
            time_done: Some(now.naive_local()),
            next_act: Some(self.fsm.current()),
            external_id: Some(Uuid::new_v4().to_string()),
            remote_code: Some(err_code::GENERAL_CODE),
            ..self.result.create_action_default()?
        };

        let (round, action) = if let Some(p) = promo_on {
            let total = self.result.total();
            let (r, a, stats) = self
                .type_repo
                .store_bcf_safe(
                    Update(fugaso_round::ActiveModel {
                        id: Unchanged(self.round.id),
                        timestamp_close: Set(time_close),
                        win: Set(Some(total)),
                        ..self.round.clone_active_model()
                    }),
                    Insert(action.into_active_model()),
                    SqlSafeAction::Update(p),
                )
                .await
                .map_err(|e| err_on!(e))?;
            self.promo_service.commit(None, Some(stats));
            (r, a)
        } else {
            let total = self.result.total();
            let (r, a) = self
                .type_repo
                .store_bc(
                    Update(fugaso_round::ActiveModel {
                        id: Unchanged(self.round.id),
                        timestamp_close: Set(time_close),
                        win: Set(Some(total)),
                        ..self.round.clone_active_model()
                    }),
                    Insert(action.into_active_model()),
                )
                .await
                .map_err(|e| err_on!(e))?;
            (r, a)
        };
        let response = self.math.collect(
            &self.input.request,
            SpinArg {
                balance,
                round_id: self.round.common_id.ok_or_else(|| err_on!("common id is none!"))?,
                round_type: self.round.detail.clone(),
                round_multiplier: self.round.multi,
                next_act: self.fsm.current(),
                promo: self.result.promo(),
                stake: 0,
            },
        )?;
        self.round = round.clone();
        let status = self.check_round_status(&action.next_act);
        Ok((Response::GameData(Arc::new(response)), round, action, status, self.input.promo_value.clone()))
    }

    pub async fn close_round(&mut self) -> Result<(Response<M::Special, M::Restore>, Round, Action), ServerError> {
        self.fsm.server_act(ActionKind::CLOSE)?;
        let result = self.math.close(self.fsm.current())?;
        self.result = Arc::new(result);
        let now = Local::now();
        let action = Action {
            id: self.table_id_gen.gen_action().await.map_err(|e| err_on!(e))?,
            amount: 0,
            act_descr: Some(ActionKind::CLOSE),
            round_id: Some(self.round.id),
            time_done: Some(now.naive_local()),
            next_act: Some(self.fsm.current()),
            external_id: Some(Uuid::new_v4().to_string()),
            remote_code: Some(err_code::GENERAL_CODE),
            ..self.result.create_action_default()?
        };
        let (r, a) = self
            .type_repo
            .store_bc(
                Update(fugaso_round::ActiveModel {
                    timestamp_close: Set(Some(now.naive_local())),
                    ..self.round.clone_active_model()
                }),
                Insert(action.into_active_model()),
            )
            .await
            .map_err(|e| err_on!(e))?;
        self.round = r.clone();
        Ok((Response::GameData(self.result.clone()), r, a))
    }

    pub async fn find_error_round(&self) -> Result<Option<(Round, Vec<Action>)>, ServerError> {
        let last = self.round_repo.find_last_round(self.user_id, self.game.0, RoundStatus::REMOTE_ERROR).await.map_err(|e| err_on!(e))?;
        Ok(last.filter(|r| {
            r.1.len() > 0 && (r.1[0].act_descr == Some(ActionKind::COLLECT) || r.1[0].act_descr == Some(ActionKind::FREE_COLLECT) || r.1[0].act_descr == Some(ActionKind::CLOSE))
        }))
    }

    pub async fn find_promo(&self, round: &Round) -> Result<(PromoValue, Promo), ServerError> {
        let common_id = round.common_id.ok_or_else(|| err_on!("common_id is none!"))?;
        self.promo_service.find_promo(common_id).await
    }

    pub fn check_round_status(&self, kind: &Option<ActionKind>) -> GameStatus {
        match kind {
            None => GameStatus::pending,
            Some(k) => {
                if k == &ActionKind::BET {
                    GameStatus::completed
                } else {
                    GameStatus::pending
                }
            }
        }
    }

    pub async fn round_result(&mut self, balance: Decimal) -> Result<(), ServerError> {
        self.round = self
            .type_repo
            .store_b(Update(fugaso_round::ActiveModel {
                balance: Set(Some(balance)),
                ..self.round.clone_active_model()
            }))
            .await
            .map_err(|e| err_on!(e))?;
        Ok(())
    }

    pub async fn fix(&mut self, action_id: i64, round_id: i64, balance: Decimal) -> Result<(), ServerError> {
        self.type_repo
            .store_bc(
                Update(fugaso_round::ActiveModel {
                    id: Unchanged(round_id),
                    status: Set(Some(RoundStatus::SUCCESS)),
                    timestamp_close: Set(Some(Local::now().naive_local())),
                    balance: Set(Some(balance)),
                    ..fugaso_round::Model::unchanged_active_model()
                }),
                Update(fugaso_action::ActiveModel {
                    id: Unchanged(action_id),
                    remote_code: Set(Some(err_code::GENERAL_CODE)),
                    error_info: Set(None),
                    ..fugaso_action::Model::unchanged_active_model()
                }),
            )
            .await
            .map_err(|e| err_on!(e))?;
        Ok(())
    }

    pub async fn on_error(&mut self, action_id: i64, round_id: i64, error: &AccountError, status: RoundStatus) -> Result<(), ServerError> {
        self.type_repo
            .store_bc(
                Update(fugaso_round::ActiveModel {
                    id: Unchanged(round_id),
                    status: Set(Some(status)),
                    ..fugaso_round::Model::unchanged_active_model()
                }),
                Update(fugaso_action::ActiveModel {
                    id: Unchanged(action_id),
                    remote_code: Set(Some(error.rc)),
                    error_info: Set(Some(error.message.clone())),
                    ..fugaso_action::Model::unchanged_active_model()
                }),
            )
            .await
            .map_err(|e| err_on!(e))?;
        self.fsm.reset(ActionKind::SPIN);
        Ok(())
    }

    pub fn is_end(&self) -> bool {
        self.fsm.current() == ActionKind::CLOSE
    }

    pub fn is_collect(&self) -> bool {
        return self.fsm.current() == ActionKind::COLLECT || self.fsm.current() == ActionKind::GAMBLE_END;
    }

    pub fn is_free_collect(&self) -> bool {
        return self.fsm.current() == ActionKind::FREE_COLLECT || self.fsm.current() == ActionKind::GAMBLE_FREE_END;
    }

    pub async fn history(&self, r: HistoryRequest) -> Result<Vec<RoundStory<M::Special, M::Restore>>, ServerError> {
        let limit = if r.limit > self.config.history_limit {
            self.config.history_limit
        } else {
            r.limit
        };
        let rounds = self.round_repo.find_last_rounds(self.user_id, self.game.0, limit).await.map_err(|e| err_on!(e))?;

        let mut story = rounds
            .into_iter()
            .map(|r| {
                let req_on = self.validator.from_round(&r.0, self.input.request.reels());
                let mut r_on: RoundStory<M::Special, M::Restore> = r.into();
                r_on.game_name = self.game.1.clone();
                r_on.stake_on = self.bet_calculator.calc_total_bet(&req_on);
                r_on.actions.sort_by(|a1, a2| a1.id.cmp(&a2.id).reverse());
                r_on
            })
            .collect::<Vec<RoundStory<_, _>>>();
        story.sort_by(|r1, r2| r1.date_start.cmp(&r2.date_start).reverse());

        Ok(story)
    }

    pub fn apply_tournaments(&self, mut rounds: Vec<RoundStory<M::Special, M::Restore>>, gains: Vec<TournamentUserWin>) -> Vec<RoundStory<M::Special, M::Restore>> {
        let mut gain_map: HashMap<String, Vec<_>> = gains.into_iter().fold(HashMap::new(), |mut acc, v| {
            if let Some(vec) = acc.get_mut(&v.round_id) {
                vec.push(v);
            } else {
                acc.insert(v.round_id.clone(), vec![v]);
            }
            acc
        });
        rounds.iter_mut().for_each(|r| {
            let win: Option<i64> = gain_map.get(&r.id.to_string()).map(|v| v.iter().map(|t| t.amount * Decimal::new(100, 0)).map(|t| t.to_i64().unwrap_or(0)).sum());
            if let Some(w) = win {
                r.win = Some(r.win.unwrap_or(0) + w);
            }
            r.actions.iter_mut().for_each(|a| {
                if a.description == Some(ActionKind::BET) {
                    if let Some(t) = gain_map.remove(&r.id.to_string()) {
                        a.tournaments = t;
                    }
                }
            });
        });
        rounds
    }

    pub async fn close(&self) {
        self.type_repo.flush().await;
    }
}

pub struct ReplayAdmin<M: SlotMath> {
    pub round_repo: Arc<RoundRepository>,
    pub game_service: Arc<GameService>,
    pub math: ReplayMath<M>,
}

impl<M: SlotMath> ReplayAdmin<M>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: 'static,
{
    pub async fn load(&mut self, round: fugaso_round::Model, actions: Vec<fugaso_action::Model>) -> Result<Decimal, ServerError> {
        let balance = self.math.load(round, actions)?;
        Ok(balance)
    }

    pub fn next(&mut self) -> Result<Response<M::Special, M::Restore>, ServerError> {
        self.math.next().map(|d| Response::GameData(d))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSettings {
    #[serde(default = "StepSettings::default_win")]
    pub win: Decimal,
    pub take: usize,
}

impl StepSettings {
    fn default_win() -> Decimal {
        Decimal::new(i64::MAX, 0)
    }
}

impl Default for StepSettings {
    fn default() -> Self {
        Self {
            win: Decimal::new(i64::MAX, 0),
            take: 100,
        }
    }
}

impl StepSettings {
    fn convert(&self, rate: Decimal) -> Step {
        let take = if self.take < math::MIN_TAKE {
            warn!("illegal take - {} change to {}", self.take, math::MIN_TAKE);
            math::MIN_TAKE
        } else if self.take > math::MAX_TAKE {
            warn!("illegal take - {} change to {}", self.take, math::MAX_TAKE);
            math::MAX_TAKE
        } else {
            self.take
        };
        let max = if self.win < Decimal::new(math::MIN_ALLOWED_EUR, 0) {
            warn!("illegal allowed - {} change to {}", self.take, Decimal::new(math::MIN_ALLOWED_EUR, 0));
            (Decimal::new(math::MIN_ALLOWED_EUR, 0) / rate * Decimal::new(100, 0)).to_i64().unwrap_or(i64::MAX)
        } else {
            (self.win / rate * Decimal::new(100, 0)).to_i64().unwrap_or(i64::MAX)
        };
        Step {
            win: max,
            take,
        }
    }
}

pub trait StoreService {
    fn start_round(
        &self,
        _promo_state: (Option<promo_transaction::Model>, Option<promo_account::ActiveModel>, Option<promo_stats::ActiveModel>),
        action: fugaso_action::ActiveModel,
        round: fugaso_round::ActiveModel,
    ) -> Result<(fugaso_action::Model, fugaso_round::Model), ServerError> {
        let r = round.try_into_model().map_err(|e| err_on!(e))?;
        let a = action.try_into_model().map_err(|e| err_on!(e))?;

        Ok((a, r))
    }
}

pub trait StoreServiceFactory {
    fn create_store_service(&self, demo: bool) -> Box<dyn StoreService + Send + Sync>;
}

pub struct DemoStoreService;

impl StoreService for DemoStoreService {}

pub struct RealStoreService;

impl StoreService for RealStoreService {}

pub struct BetConfigurator {
    exchange_repo: Arc<ExchangeRateRepository>,
    map_bets: HashMap<String, GameConfig>,
    euro: currency::Model,
}

impl BetConfigurator {
    pub async fn new(currency_repo: Arc<CurrencyRepository>, exchange_repo: Arc<ExchangeRateRepository>, json: &str) -> Result<Self, ServerError> {
        let euro = currency_repo.find_by_code("EUR").await.map_err(|e| err_on!(e))?.ok_or_else(|| err_on!(EURO_CURRENCY_ERROR))?;
        let list: Vec<BetConfig> = serde_json::from_str(&json).map_err(|e| err_on!(e))?;

        let map_bets = list
            .into_iter()
            .map(|v| {
                let settings = RequestSettings {
                    bets: v.bets,
                    denomination: v.denomination,
                    index_default: v.index_default,
                    rate: Decimal::ONE,
                    line_default: v.line_default,
                };
                let exclusion = v
                    .currency_exclusion
                    .into_iter()
                    .map(|ex| {
                        let req_currency = RequestSettings {
                            bets: ex.bets,
                            denomination: ex.denomination,
                            index_default: ex.index_default,
                            rate: Decimal::ONE,
                            line_default: ex.line_default,
                        };
                        ex.currency.into_iter().map(move |c| (c, req_currency.clone()))
                    })
                    .flat_map(|l| l)
                    .collect::<HashMap<String, RequestSettings>>();
                let country_exclusion = v
                    .country_exclusion
                    .into_iter()
                    .map(|ex| {
                        let req_country = RequestSettings {
                            bets: ex.bets,
                            denomination: ex.denomination,
                            index_default: ex.index_default,
                            rate: Decimal::ONE,
                            line_default: ex.line_default,
                        };
                        ex.country.into_iter().map(move |c| (c, req_country.clone()))
                    })
                    .flat_map(|l| l)
                    .collect::<HashMap<String, RequestSettings>>();
                (
                    v.clazz,
                    GameConfig {
                        settings,
                        exclusion,
                        country_exclusion,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(BetConfigurator {
            exchange_repo,
            map_bets,
            euro,
        })
    }

    pub fn get_default_settings(&self, math_class: &str, currency: &(i64, String), country: Option<String>) -> Result<&RequestSettings, ServerError> {
        let config = self.map_bets.get(math_class).ok_or_else(|| err_on!(GAME_BET_ERROR))?;
        let req_simple = &config.settings;
        let request_on =
            config.exclusion.get(&currency.1).unwrap_or(config.country_exclusion.get(&country.map(|v| v.to_uppercase()).unwrap_or("".to_string())).unwrap_or(req_simple));
        Ok(request_on)
    }

    pub async fn find_bets<C: BetCalculator>(&self, game_settings: GameSettings<C>, currency: &(i64, String), country: Option<String>) -> Result<RequestSettings, ServerError> {
        let request_on = self.get_default_settings(&game_settings.math_class, currency, country)?;
        self.filter(game_settings, request_on, currency).await
    }

    pub async fn filter<C: BetCalculator>(&self, game_settings: GameSettings<C>, request_on: &RequestSettings, currency: &(i64, String)) -> Result<RequestSettings, ServerError> {
        let exchange = if currency.1 == "EUR" {
            exchange_rate::Model {
                id: 0,
                rate: Decimal::ONE,
                coefficient: 1,
                ..Default::default()
            }
        } else {
            self.exchange_repo.find_by_src_dest(currency.0, self.euro.id).await.map_err(|e| err_on!(e))?.ok_or_else(|| err_on!(exchange_msg(currency.0)))?
        };
        let max_win_converted = game_settings.max_win * Decimal::new(100, 0) / exchange.rate;
        let max_stake_converted = game_settings.max_stake * Decimal::new(100, 0) / exchange.rate;
        let request = if request_on.denomination == vec![1]
            || request_on.denomination == vec![1, 2]
            || request_on.denomination == vec![20]
            || request_on.denomination == vec![25]
            || request_on.denomination == vec![50]
            || request_on.denomination == vec![10]
        {
            let denom_max = *request_on.denomination.last().ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
            let bets_modified = request_on
                .bets
                .iter()
                .map(|b| b * exchange.coefficient)
                .filter(|b| {
                    let stake = game_settings.get_stake(*b, denom_max);
                    let max_gain = Decimal::new(stake * game_settings.exposure as i64, 0);
                    Decimal::new(stake, 0) <= max_stake_converted && max_gain <= max_win_converted
                })
                .collect::<Vec<_>>();
            RequestSettings {
                bets: bets_modified,
                denomination: request_on.denomination.clone(),
                index_default: request_on.index_default,
                rate: exchange.rate,
                line_default: request_on.line_default,
            }
        } else {
            let bet_max = *request_on.bets.last().ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
            let denom_modified = request_on
                .denomination
                .iter()
                .map(|d| d * exchange.coefficient)
                .filter(|d| {
                    let stake = game_settings.get_stake(bet_max, *d);
                    let max_gain = Decimal::new(stake * game_settings.exposure as i64, 0);
                    Decimal::new(stake, 0) <= max_stake_converted && max_gain <= max_win_converted
                })
                .collect::<Vec<_>>();
            RequestSettings {
                bets: request_on.bets.clone(),
                denomination: denom_modified,
                index_default: request_on.index_default,
                line_default: request_on.line_default,
                rate: exchange.rate,
            }
        };

        Ok(request)
    }
}

pub struct GameSettings<C: BetCalculator> {
    calculator: C,
    math_class: String,
    exposure: i32,
    max_win: Decimal,
    max_stake: Decimal,
    max_line: usize,
}

impl<C: BetCalculator> GameSettings<C> {
    pub fn new(calculator: C, lines: Vec<usize>, math_class: String, exposure: i32, bet_settings: &BetSettings) -> Result<Self, ServerError> {
        let exposure_on = if exposure > 0 {
            exposure
        } else {
            1
        };
        let max_win_euro = if bet_settings.max_win > Decimal::ZERO {
            bet_settings.max_win
        } else {
            Decimal::new(i32::MAX as i64, 2)
        };
        let max_stake = if bet_settings.max_stake > Decimal::ZERO {
            bet_settings.max_stake
        } else {
            Decimal::new(i32::MAX as i64, 2)
        };
        if lines.is_empty() {
            return Err(err_on!(ILLEGAL_ARGUMENT));
        }

        Ok(Self {
            calculator,
            max_line: lines[lines.len() - 1],
            math_class,
            exposure: exposure_on,
            max_win: max_win_euro,
            max_stake,
        })
    }
    pub fn get_stake(&self, bet: i32, denomination: i32) -> i64 {
        let request = C::I::create_input(bet, self.max_line, denomination, 1);
        return self.calculator.calc_total_bet(&request);
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RequestSettings {
    pub bets: Vec<i32>,
    pub denomination: Vec<i32>,
    pub index_default: usize,
    pub rate: Decimal,
    pub line_default: Option<usize>,
}

impl RequestSettings {
    pub fn from_db(bets: &Option<String>, denomination: &Option<String>, index_default: usize, line_defalut: Option<usize>) -> Result<Self, ServerError> {
        let bet_st = bets.as_ref().ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
        let d_st = denomination.as_ref().ok_or_else(|| err_on!(ILLEGAL_ARGUMENT))?;
        let bets_on = bet_st.split(",").map(|s| s.parse::<i32>()).collect::<Result<Vec<_>, _>>()?;
        let denom_on = d_st.split(",").map(|s| s.parse::<i32>()).collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            bets: bets_on,
            denomination: denom_on,
            index_default,
            rate: Decimal::ONE,
            line_default: line_defalut,
        })
    }

    pub fn bets_str(&self) -> String {
        self.bets.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
    }

    pub fn denom_str(&self) -> String {
        self.denomination.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameConfig {
    pub settings: RequestSettings,
    pub exclusion: HashMap<String, RequestSettings>,
    pub country_exclusion: HashMap<String, RequestSettings>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetConfig {
    pub clazz: String,
    pub bets: Vec<i32>,
    pub denomination: Vec<i32>,
    pub index_default: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_default: Option<usize>,
    #[serde(default)]
    pub currency_exclusion: Vec<CurrencyExclusion>,
    #[serde(default)]
    pub country_exclusion: Vec<CountryExclusion>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyExclusion {
    pub index_default: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_default: Option<usize>,
    pub currency: Vec<String>,
    pub bets: Vec<i32>,
    pub denomination: Vec<i32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryExclusion {
    pub index_default: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_default: Option<usize>,
    pub country: Vec<String>,
    pub bets: Vec<i32>,
    pub denomination: Vec<i32>,
}
