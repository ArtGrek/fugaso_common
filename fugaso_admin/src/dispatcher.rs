use async_trait::async_trait;
use chrono::Local;
use essential_async::channel;
use essential_async::channel::OneShotSender;
use essential_core::account_service::{acc_err, err_code, AccountServiceFactory, ErrorType, GameStatus};
use essential_core::digest::calc_hmac_sha256;
use essential_core::err_on;
use essential_core::error::ServerError;
use essential_data::repo::JackpotRepository;
use fugaso_core::admin::{InitArg, SlotAdmin, StateLoader, TypedRepoFactory};
use fugaso_core::protocol::{AdminError, ErrorData, HistoryData, HistoryRequest, IResponse, LoginRequest, PlayerError, PlayerRequest, Response, TournamentData};
use fugaso_core::proxy::{is_rollback_code, AuthData, JackpotProxyFactory, PromoServiceFactory, PromoValue, RetryServiceFactory, SlotProxy};
use fugaso_core::tournament::{TournamentPlace, TournamentWinData};
use fugaso_data::fugaso_round::{self, RoundStatus};
use fugaso_data::sequence_generator::IdGeneratorFactory;
use fugaso_data::{fugaso_action, tournament_gain};
use fugaso_data::{fugaso_action::Model as Action, fugaso_round::Model as Round};
use fugaso_math::math::SlotMath;
use fugaso_math::protocol::{id, ReSpinInfo, RestoreInfo};
use log::{debug, error, info};
use sea_orm::prelude::{Decimal, Uuid};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, oneshot};

pub struct JackpotDispatcher {
    pub sender: UnboundedSender<(Vec<i64>, oneshot::Sender<HashMap<String, Decimal>>)>,
}

impl JackpotDispatcher {
    pub fn new(jackpot_repo: Arc<JackpotRepository>, duration: Duration) -> Self {
        let (s, mut r) = mpsc::unbounded_channel::<(Vec<i64>, oneshot::Sender<HashMap<String, Decimal>>)>();
        tokio::spawn(async move {
            let mut cache = JackpotHolder::new(jackpot_repo);
            while let Some(e) = r.recv().await {
                let map = cache.get(&e.0, duration);
                if let Some(m) = map {
                    if let Err(_) = e.1.send(m.clone()) {
                        error!("error send {:?}", e.0)
                    }
                } else {
                    let r = cache.load(e.0).await;
                    match r {
                        Ok(m) => {
                            if let Err(_) = e.1.send(m.clone()) {
                                error!("error send jackpot answer")
                            }
                        }
                        Err(e) => error!("error {e}"),
                    }
                }
            }
        });
        Self {
            sender: s,
        }
    }

    pub async fn handle_jackpots(&self, key: Vec<i64>) {
        let (s, rec) = oneshot::channel();
        let result = self.sender.send((key, s));
        match result {
            Ok(_) => match rec.await {
                Ok(j) => j,
                Err(e) => {
                    error!("error receive jackpots: {e}");
                    HashMap::new()
                }
            },
            Err(e) => {
                error!("Error write jackpot id: {}", e.to_string());
                HashMap::new()
            }
        };
    }
}

pub struct JackpotHolder {
    cache: HashMap<Vec<i64>, (Instant, HashMap<String, Decimal>)>,
    jackpot_repo: Arc<JackpotRepository>,
}

impl JackpotHolder {
    pub fn new(jackpot_repo: Arc<JackpotRepository>) -> Self {
        Self {
            cache: HashMap::new(),
            jackpot_repo,
        }
    }
    pub fn get(&self, key: &Vec<i64>, duration: Duration) -> Option<&HashMap<String, Decimal>> {
        self.cache.get(key).filter(|p| p.0.elapsed() < duration).map(|p| &p.1)
    }
    pub async fn load(&mut self, key: Vec<i64>) -> Result<HashMap<String, Decimal>, ServerError> {
        let jackpots = self.jackpot_repo.find_by_ids(key.clone()).await.map_err(|e| err_on!(e))?;
        let m = jackpots.into_iter().map(|j| (j.name, j.contributions)).collect::<HashMap<_, _>>();
        self.cache.insert(key, (Instant::now(), m.clone()));
        Ok(m)
    }
}

pub enum ClientEvent {
    Request(ClientRequest),
    Balance(Decimal),
    TournamentWin(TournamentWinData),
    Stop(Option<OneShotSender<bool>>, Option<String>),
}

pub struct ClientRequest(pub Option<Uuid>, pub serde_json::Value, pub OneShotSender<ResponseStacked>);

pub struct DispatcherHolder {
    access_time: Instant,
    sender: channel::UnboundedSender<ClientEvent>,
}

impl DispatcherHolder {
    pub async fn new(mut dispatcher: Box<dyn Dispatcher + Send + Sync>) -> Result<Self, ErrorData> {
        let (s, mut r) = mpsc::unbounded_channel::<ClientEvent>();
        tokio::spawn(async move {
            while let Some(event) = r.recv().await {
                match event {
                    ClientEvent::Request(req) => {
                        let response = dispatcher.handle(req.0, req.1).await;
                        req.2.send(response, file!(), line!());
                    }
                    ClientEvent::Stop(s, game_session_id) => {
                        dispatcher.disconnect(s, game_session_id).await;
                        break;
                    }
                    ClientEvent::Balance(b) => {
                        if let Err(e) = dispatcher.on_balance(b) {
                            error!("error set balance {e:?}!");
                        }
                    }
                    ClientEvent::TournamentWin(win) => {
                        dispatcher.on_tournament_win(win);
                    }
                }
            }
        });
        Ok(Self {
            sender: channel::UnboundedSender::new(s),
            access_time: Instant::now(),
        })
    }

    pub fn ping(&mut self) {
        self.access_time = Instant::now();
    }

    pub fn send(&mut self, event: ClientEvent) {
        self.access_time = Instant::now();
        self.sender.send(event, file!(), line!());
    }

    pub fn sender(&self) -> UnboundedSender<ClientEvent> {
        self.sender.inner.clone()
    }

    pub fn stop_if_dead(&self, duration: std::time::Duration) -> bool {
        let now = Instant::now();
        if now - self.access_time > duration {
            self.sender.send(ClientEvent::Stop(None, None), file!(), line!());
            true
        } else {
            false
        }
    }
}

pub enum PlayerEvent {
    Clean,
    Ping {
        token: String,
    },
    Request(String, ClientRequest),
    Disconnect {
        id: i64,
        game_session_id: Option<String>,
        send_close: OneShotSender<bool>,
    },
    Register {
        token: String,
        holder: DispatcherHolder,
        id: i64,
    },
    Sender {
        token: String,
        sender: OneShotSender<mpsc::UnboundedSender<ClientEvent>>,
    },
    Online(OneShotSender<OnlineHolder>),
    State(OneShotSender<StateHolder>),
    TournamentWin {
        id: i64,
        gain: tournament_gain::Model,
        balance: Decimal,
        winners: Arc<Vec<TournamentPlace>>,
        award_id: i64,
    },
}

#[derive(Debug)]
pub struct TournamentEventWin {
    pub winners: HashMap<Uuid, Arc<Vec<TournamentPlace>>>,
    pub gains: Vec<tournament_gain::Model>,
    pub balance_user: HashMap<Uuid, (Uuid, Decimal, i64)>,
}

#[async_trait]
pub trait IDispacthercontext {
    async fn register(
        &self,
        mut dispatcher: Box<dyn Dispatcher + Send + Sync>,
        request: LoginRequest,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(ResponseStacked, String), ErrorData>;

    async fn send(&self, req: (String, Option<Uuid>, serde_json::Value)) -> Result<ResponseStacked, ErrorData>;

    async fn online(&self) -> Result<OnlineHolder, ErrorData>;

    async fn state(&self) -> Result<StateHolder, ErrorData>;

    async fn ping(&self, token: String);

    fn tournament_win(&self, event: TournamentEventWin);
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatcherConfig {
    clean_sec: u64,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            clean_sec: 60 * 60,
        }
    }
}

impl DispatcherConfig {
    pub fn clean_duration(&self) -> Duration {
        Duration::from_secs(self.clean_sec)
    }
}

pub struct DispatcherContext {
    sender: channel::UnboundedSender<PlayerEvent>,
}

impl DispatcherContext {
    pub const SECRET_KEY: &'static str = "Wp&!CsFrfgn8syfxA*Sm";
    pub fn new(cfg: &DispatcherConfig) -> Self {
        let (s, mut r) = mpsc::unbounded_channel::<PlayerEvent>();
        let duration_clean = cfg.clean_duration();
        tokio::spawn(async move {
            let mut map_holder: HashMap<i64, (String, DispatcherHolder)> = HashMap::new();
            let mut map_session: HashMap<String, i64> = HashMap::new();
            while let Some(p) = r.recv().await {
                match p {
                    PlayerEvent::Clean => {
                        info!("clean run - sessions:{} names:{}", map_session.len(), map_holder.len());
                        map_holder.retain(|_k, v| !v.1.stop_if_dead(duration_clean));
                        map_session.retain(|_k, v| map_holder.contains_key(v));
                        info!("clean end - sessions:{} names:{}", map_session.len(), map_holder.len());
                    }
                    PlayerEvent::Online(sender) => {
                        let now = Instant::now();
                        let count = map_holder.values().filter(|v| now - v.1.access_time <= Duration::from_secs(60)).count();
                        sender.send(
                            OnlineHolder {
                                count,
                            },
                            file!(),
                            line!(),
                        );
                    }
                    PlayerEvent::State(sender) => {
                        sender.send(
                            StateHolder {
                                sessions: map_session.len(),
                                clients: map_holder.len(),
                            },
                            file!(),
                            line!(),
                        );
                    }
                    PlayerEvent::Request(t, request) => {
                        let holder = map_session.get(&t).map(|n| map_holder.get_mut(n));
                        if let Some(Some(h)) = holder {
                            h.1.send(ClientEvent::Request(request));
                        } else {
                            request.2.send(
                                ResponseStacked {
                                    id: None,
                                    answer: Box::new(vec![Response::Error::<ReSpinInfo, RestoreInfo>(ErrorData {
                                        message: err_code::NOT_LOGGED_ON.3.to_string(),
                                        code: err_code::NOT_LOGGED_ON.2,
                                    })]),
                                    cache: false,
                                },
                                file!(),
                                line!(),
                            );
                        };
                    }
                    PlayerEvent::Register {
                        token,
                        holder,
                        id,
                    } => {
                        map_session.insert(token.clone(), id);
                        if let Some(h) = map_holder.insert(id, (token, holder)) {
                            map_session.remove(&h.0);
                        }
                    }
                    PlayerEvent::Disconnect {
                        id,
                        send_close,
                        game_session_id,
                    } => {
                        if let Some(h) = map_holder.get_mut(&id) {
                            h.1.send(ClientEvent::Stop(Some(send_close), game_session_id));
                        } else {
                            send_close.send(false, file!(), line!());
                        }
                    }
                    PlayerEvent::Sender {
                        token,
                        sender,
                    } => {
                        let holder = map_session.get(&token).map(|n| map_holder.get_mut(n));
                        if let Some(Some(h)) = holder {
                            sender.send(h.1.sender.inner.clone(), file!(), line!());
                        };
                    }
                    PlayerEvent::Ping {
                        token,
                    } => {
                        let holder = map_session.get(&token).map(|n| map_holder.get_mut(n));
                        if let Some(Some(h)) = holder {
                            h.1.ping();
                        }
                    }
                    PlayerEvent::TournamentWin {
                        id,
                        gain,
                        balance,
                        winners,
                        award_id,
                    } => {
                        if let Some(h) = map_holder.get_mut(&id) {
                            h.1.send(ClientEvent::TournamentWin(TournamentWinData {
                                winners,
                                gain,
                                balance,
                                award_id,
                            }))
                        }
                    }
                }
            }
        });

        let sender_timer = s.clone();
        tokio::spawn(async move {
            let now = tokio::time::Instant::now() + duration_clean;
            let mut interval = tokio::time::interval_at(now, duration_clean);

            loop {
                interval.tick().await;
                if let Err(_) = sender_timer.send(PlayerEvent::Clean) {
                    error!("error clean")
                }
            }
        });
        Self {
            sender: channel::UnboundedSender::new(s),
        }
    }

    pub async fn holder_sender(&self, token: String) -> Result<mpsc::UnboundedSender<ClientEvent>, String> {
        let (sender, receiver) = oneshot::channel();
        self.sender.send(
            PlayerEvent::Sender {
                token,
                sender: OneShotSender::new(sender),
            },
            file!(),
            line!(),
        );
        match receiver.await {
            Ok(sender) => Ok(sender),
            Err(_) => Err("error get holder".to_string()),
        }
    }
}

#[async_trait]
impl IDispacthercontext for DispatcherContext {
    async fn register(
        &self,
        mut dispatcher: Box<dyn Dispatcher + Send + Sync>,
        request: LoginRequest,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(ResponseStacked, String), ErrorData> {
        let (id, game_session_id) = dispatcher
            .login(
                AuthData {
                    user_name: request.session.user_name,
                    session_id: request.session.session_id,
                    operator_id: request.session.operator_id,
                    game_name: request.session.game_name,
                    mode: request.session.mode,
                    password: request.password,
                },
                ip_address_list,
                user_agent,
                round_actions.clone(),
            )
            .await?;
        let (send_close, rec_close) = oneshot::channel();
        self.sender.send(
            PlayerEvent::Disconnect {
                id,
                send_close: OneShotSender::new(send_close),
                game_session_id,
            },
            file!(),
            line!(),
        );
        match rec_close.await {
            Err(_) => error!("error wait disconnect!"),
            _ => {}
        };
        let response = dispatcher.join(id, request.country, round_actions).await?;
        let holder = DispatcherHolder::new(dispatcher).await?;
        let now = Local::now();
        let token = calc_hmac_sha256(Self::SECRET_KEY, &format!("{}:{}", id, now.timestamp_millis()))?;
        match self.sender.send_on(PlayerEvent::Register {
            token: token.clone(),
            holder,
            id,
        }) {
            Ok(_) => Ok((response, token)),
            Err(_) => {
                error!("error register dispatcher");
                Err(ErrorData {
                    message: "Unknown error".to_string(),
                    code: ErrorType::UNKNOWN,
                })
            }
        }
    }

    async fn send(&self, req: (String, Option<Uuid>, serde_json::Value)) -> Result<ResponseStacked, ErrorData> {
        let (s, r) = oneshot::channel();
        self.sender.send(PlayerEvent::Request(req.0, ClientRequest(req.1, req.2, OneShotSender::new(s))), file!(), line!());
        match r.await {
            Ok(r) => Ok(r),
            Err(_) => {
                error!("error answer holder");
                Err(ErrorData {
                    message: "Unknown error".to_string(),
                    code: ErrorType::UNKNOWN,
                })
            }
        }
    }

    async fn online(&self) -> Result<OnlineHolder, ErrorData> {
        let (sender, receiver) = oneshot::channel();
        self.sender.send(PlayerEvent::Online(OneShotSender::new(sender)), file!(), line!());
        match receiver.await {
            Ok(sender) => Ok(sender),
            Err(_) => Err(ErrorData {
                message: "Unknown error".to_string(),
                code: ErrorType::UNKNOWN,
            }),
        }
    }

    async fn state(&self) -> Result<StateHolder, ErrorData> {
        let (sender, receiver) = oneshot::channel();
        self.sender.send(PlayerEvent::State(OneShotSender::new(sender)), file!(), line!());
        match receiver.await {
            Ok(sender) => Ok(sender),
            Err(_) => Err(ErrorData {
                message: "Unknown error".to_string(),
                code: ErrorType::UNKNOWN,
            }),
        }
    }

    async fn ping(&self, token: String) {
        self.sender.send(
            PlayerEvent::Ping {
                token,
            },
            file!(),
            line!(),
        );
    }

    fn tournament_win(&self, event: TournamentEventWin) {
        debug!("tournament win: {event:?}");
        for g in event.gains {
            if let Some(a) = event.balance_user.get(&g.inbound_id) {
                let winners = if let Some(winners) = event.winners.get(&a.0) {
                    Arc::clone(winners)
                } else {
                    Arc::new(vec![])
                };
                self.sender.send(
                    PlayerEvent::TournamentWin {
                        id: g.user_id,
                        gain: g,
                        balance: a.1,
                        winners,
                        award_id: a.2,
                    },
                    file!(),
                    line!(),
                )
            }
        }
    }
}

pub struct ResponseStacked {
    pub id: Option<Uuid>,
    pub answer: Box<dyn IResponse + Send + Sync>,
    pub cache: bool,
}

#[async_trait]
pub trait Dispatcher {
    async fn login(
        &mut self,
        r: AuthData,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(i64, Option<String>), ErrorData>;

    async fn join(&mut self, user_id: i64, country: Option<String>, round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>) -> Result<ResponseStacked, ErrorData>;

    async fn handle(&mut self, request_id: Option<Uuid>, request: serde_json::Value) -> ResponseStacked;

    async fn disconnect(&mut self, send_close: Option<OneShotSender<bool>>, game_session_id: Option<String>);

    fn on_balance(&mut self, balance: Decimal) -> Result<(), PlayerError>;

    fn on_tournament_win(&mut self, win: TournamentWinData);
}

pub struct DisconnectResult {
    pub round: Round,
    pub action: Action,
    pub status: GameStatus,
    pub promo: PromoValue,
}

#[async_trait]
pub trait SlotBaseDispatcher
where
    <<Self as SlotBaseDispatcher>::M as SlotMath>::Input: Send + Sync,
{
    type M: SlotMath;
    type Parent: SlotBaseDispatcher<M = Self::M> + Send + Sync;

    fn parent(&self) -> &Self::Parent;

    fn parent_mut(&mut self) -> &mut Self::Parent;

    async fn login(
        &mut self,
        r: AuthData,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(i64, Option<String>), ErrorData> {
        let parent = self.parent_mut();
        parent.login(r, ip_address_list, user_agent, round_actions).await
    }

    async fn join(&mut self, user_id: i64, country: Option<String>, round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>) -> Result<ResponseStacked, ErrorData> {
        let parent = self.parent_mut();
        parent.join(user_id, country, round_actions).await
    }

    fn is_wrong_id(&self, id: Uuid) -> bool {
        let parent = self.parent();
        parent.is_wrong_id(id)
    }

    async fn on_error(&mut self, e: PlayerError) -> ErrorData {
        let parent = self.parent_mut();
        parent.on_error(e).await
    }

    async fn on_history(&self, r: HistoryRequest) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent();
        parent.on_history(r).await
    }

    async fn on_tournament_info(&mut self) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent_mut();
        parent.on_tournament_info().await
    }

    async fn on_spin(&mut self, r: <<Self as SlotBaseDispatcher>::M as SlotMath>::Input) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent_mut();
        parent.on_spin(r).await
    }

    async fn on_respin(&mut self) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent_mut();
        parent.on_respin().await
    }

    async fn on_free_spin(&mut self) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent_mut();
        parent.on_free_spin().await
    }

    async fn on_collect(&mut self, game_session_id: Option<String>) -> Result<ResponseStacked, PlayerError> {
        let parent = self.parent_mut();
        parent.on_collect(game_session_id).await
    }

    async fn disconnect(&mut self, send_close: Option<OneShotSender<bool>>, game_session_id: Option<String>) {
        let parent = self.parent_mut();
        parent.disconnect(send_close, game_session_id).await
    }

    fn on_balance(&mut self, balance: Decimal) -> Result<(), PlayerError> {
        let parent = self.parent_mut();
        parent.on_balance(balance)
    }

    fn on_tournament_win(&mut self, win: TournamentWinData) {
        let parent = self.parent_mut();
        parent.on_tournament_win(win)
    }
}

pub struct SlotDispatcher<
    M: SlotMath,
    FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory,
    FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory,
    S: StateLoader,
> {
    pub proxy: SlotProxy<M, FP>,
    pub admin: SlotAdmin<M, FA, S>,
    pub next_id: Uuid,
    pub jackpot_dispatcher: Arc<JackpotDispatcher>,
    pub tournament_gains: VecDeque<TournamentWinData>,
}

impl<M: SlotMath, FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory, FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory, S: StateLoader>
    SlotDispatcher<M, FA, FP, S>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
{
    pub fn new(jackpot_dispatcher: Arc<JackpotDispatcher>, proxy: SlotProxy<M, FP>, admin: SlotAdmin<M, FA, S>) -> Self {
        Self {
            next_id: Uuid::new_v4(),
            jackpot_dispatcher,
            proxy,
            admin,
            tournament_gains: VecDeque::new(),
        }
    }
}

#[async_trait]
impl<
        M: SlotMath + Send + Sync,
        FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory + Send + Sync,
        FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory + Send + Sync,
        S: StateLoader + Send + Sync,
    > SlotBaseDispatcher for SlotDispatcher<M, FA, FP, S>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Calculator: Sync + Send + 'static,
    <M as SlotMath>::PlayFSM: Sync + Send + 'static,
    <M as SlotMath>::V: Sync + Send + 'static,
    <M as SlotMath>::Input: Sync + Send + 'static,
{
    type M = M;
    type Parent = Self;

    fn parent(&self) -> &Self::Parent {
        self
    }

    fn parent_mut(&mut self) -> &mut Self::Parent {
        self
    }

    fn is_wrong_id(&self, id: Uuid) -> bool {
        id != self.next_id
    }

    async fn login(
        &mut self,
        r: AuthData,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        _round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(i64, Option<String>), ErrorData> {
        let t = self.proxy.login(r, ip_address_list, user_agent, None).await?;
        Ok(t)
    }

    async fn join(&mut self, user_id: i64, country: Option<String>, round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>) -> Result<ResponseStacked, ErrorData> {
        self.admin
            .init(InitArg {
                user_id,
                game: self.proxy.game().clone(),
                demo: self.proxy.is_demo(),
                country,
                bet_settings: self.proxy.bet_settings.clone(),
                step_settings: self.proxy.step_settings.clone(),
                currency: self.proxy.currency()?,
                round_actions,
            })
            .await?;
        let balance = self.proxy.get_balance().await?;
        let error_round = if self.proxy.is_demo() {
            None
        } else {
            self.admin.find_error_round().await?
        };
        if let Some((r, actions)) = error_round {
            if actions.len() > 0 {
                let (promo, promo_on) = self.admin.find_promo(&r).await?;
                let last = &actions[0];
                let status = self.admin.check_round_status(&last.next_act);
                match self.proxy.result(&actions[actions.len() - 1], &r, status.clone(), promo.clone(), None).await {
                    Ok(_) => {
                        if status == GameStatus::pending {
                            self.admin.restore((r, actions), promo, promo_on, self.proxy.is_demo()).await?;
                            self.admin.round_result(self.proxy.credits).await?;
                        } else {
                            self.admin.fix(last.id, r.id, self.proxy.credits).await?;
                        }
                    }
                    Err(e) => {
                        if status == GameStatus::pending {
                            return Err(e.into());
                        }
                    }
                }
            }
        }

        let mut packets = self.proxy.join().await?;
        let join = self.admin.join(balance).await?;
        packets.push(join);
        self.next_id = Uuid::new_v4();
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(packets),
            cache: false,
        })
    }

    async fn disconnect(&mut self, send_close: Option<OneShotSender<bool>>, game_session_id: Option<String>) {
        let closed = if self.admin.is_collect() {
            if let Err(e) = self.on_collect(game_session_id).await {
                self.on_error(e).await;
                false
            } else {
                true
            }
        } else {
            false
        };
        self.admin.close().await;
        if let Some(s) = send_close {
            s.send(closed, file!(), line!());
        }
    }

    fn on_balance(&mut self, balance: Decimal) -> Result<(), PlayerError> {
        self.proxy.set_balance(balance)?;
        Ok(())
    }

    fn on_tournament_win(&mut self, win: TournamentWinData) {
        self.tournament_gains.push_back(win);
    }

    async fn on_error(&mut self, e: PlayerError) -> ErrorData {
        match e {
            PlayerError::Internal(e) => e.into(),
            PlayerError::Account(e) => e.into(),
            PlayerError::Admin(e) => match self.admin.on_error(e.action_id, e.round_id, &e.error, RoundStatus::REMOTE_ERROR).await {
                Ok(_) => e.error.into(),
                Err(inner) => inner.into(),
            },
        }
    }

    async fn on_history(&self, r: HistoryRequest) -> Result<ResponseStacked, PlayerError> {
        let rounds = self.admin.history(r).await?;
        let common_ids = rounds.iter().map(|r| r.id.to_string()).collect::<Vec<_>>();
        let gains = self.proxy.tournament_gains(common_ids).await?;
        let rounds_gains = self.admin.apply_tournaments(rounds, gains);
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(vec![Response::History::<M::Special, M::Restore>(HistoryData {
                id: id::HISTORY,
                rounds: rounds_gains,
            })]),
            cache: false,
        })
    }

    async fn on_tournament_info(&mut self) -> Result<ResponseStacked, PlayerError> {
        let tournament = self.proxy.tournament().await;
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(vec![Response::TournamentInfo::<M::Special, M::Restore>(TournamentData {
                id: id::TOURNAMENT_INFO,
                tournament,
            })]),
            cache: false,
        })
    }

    async fn on_spin(&mut self, r: M::Input) -> Result<ResponseStacked, PlayerError> {
        let (mut spin_response, round, action, promo) = self.admin.spin(self.proxy.balance(), r).await?;
        let (balance, amount) = match self.proxy.wager(&action, &round, &promo).await {
            Ok((balance, amount)) => (balance, amount),
            Err(e) => {
                if e.rc == err_code::OUT_OF_MONEY_CODE.0 {
                    self.admin.on_error(action.id, round.id, &e, RoundStatus::DECLINE).await?
                } else if is_rollback_code(e.rc) {
                    self.admin.on_error(action.id, round.id, &e, RoundStatus::ROLLBACK).await?
                } else {
                    self.admin.on_error(action.id, round.id, &e, RoundStatus::REMOTE_ERROR).await?
                }
                return Err(e.into());
            }
        };
        let (jackpot_response, jackpots) = self.proxy.check_jackpots(amount, round.id).await?;
        let tournament_win = if jackpots == 0 {
            match self.tournament_gains.pop_front() {
                None => None,
                Some(w) => self.proxy.tournament_win(w, round.common_id).await.map(|t| Response::TournamentWin::<M::Special, M::Restore>(t.into())),
            }
        } else {
            None
        };
        if self.admin.is_end() {
            let (close_response, r, a) = self.admin.close_round().await?;
            spin_response = close_response;
            let balance_result = self.proxy.result(&a, &r, GameStatus::completed, promo, None).await.map_err(|e| {
                PlayerError::Admin(AdminError {
                    round_id: r.id,
                    action_id: a.id,
                    error: e,
                })
            })?;
            self.admin.round_result(balance_result).await?;
        } else {
            self.admin.round_result(balance).await?;
        }
        self.next_id = Uuid::new_v4();
        let mut packets = vec![spin_response, jackpot_response];
        if let Some(r) = tournament_win {
            packets.push(r)
        }
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(packets),
            cache: true,
        })
    }

    async fn on_respin(&mut self) -> Result<ResponseStacked, PlayerError> {
        let (mut spin_response, _round, _action, promo) = self.admin.respin(self.proxy.balance()).await?;
        if self.admin.is_end() {
            let (close_response, r, a) = self.admin.close_round().await?;
            spin_response = close_response;
            let balance_result = self.proxy.result(&a, &r, GameStatus::completed, promo, None).await.map_err(|e| {
                PlayerError::Admin(AdminError {
                    round_id: r.id,
                    action_id: a.id,
                    error: e,
                })
            })?;
            self.admin.round_result(balance_result).await?;
        }
        self.next_id = Uuid::new_v4();
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(vec![spin_response]),
            cache: true,
        })
    }

    async fn on_free_spin(&mut self) -> Result<ResponseStacked, PlayerError> {
        let (mut spin_response, _round, _action, promo) = self.admin.free_spin(self.proxy.balance()).await?;
        if self.admin.is_end() {
            let (close_response, r, a) = self.admin.close_round().await?;
            spin_response = close_response;
            let balance_result = self.proxy.result(&a, &r, GameStatus::completed, promo, None).await.map_err(|e| {
                PlayerError::Admin(AdminError {
                    round_id: r.id,
                    action_id: a.id,
                    error: e,
                })
            })?;
            self.admin.round_result(balance_result).await?;
        }
        self.next_id = Uuid::new_v4();
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(vec![spin_response]),
            cache: true,
        })
    }

    async fn on_collect(&mut self, game_session_id: Option<String>) -> Result<ResponseStacked, PlayerError> {
        let (collect_response, round, action, status, promo) = self.admin.collect(self.proxy.balance()).await?;
        let balance_result = self.proxy.result(&action, &round, status, promo, game_session_id).await.map_err(|e| {
            PlayerError::Admin(AdminError {
                round_id: round.id,
                action_id: action.id,
                error: e,
            })
        })?;
        self.admin.round_result(balance_result).await?;
        self.next_id = Uuid::new_v4();
        Ok(ResponseStacked {
            id: Some(self.next_id),
            answer: Box::new(vec![collect_response]),
            cache: true,
        })
    }
}

#[async_trait]
impl<S: SlotBaseDispatcher + Send + Sync> Dispatcher for S
where
    <S::M as SlotMath>::Input: Send + Sync,
    <S::M as SlotMath>::Special: Serialize + Send + Sync + 'static,
    <S::M as SlotMath>::Restore: Serialize + Send + Sync + 'static,
{
    async fn login(
        &mut self,
        r: AuthData,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(i64, Option<String>), ErrorData> {
        self.login(r, ip_address_list, user_agent, round_actions).await
    }

    async fn join(&mut self, user_id: i64, country: Option<String>, round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>) -> Result<ResponseStacked, ErrorData> {
        self.join(user_id, country, round_actions).await
    }

    async fn handle(&mut self, request_id: Option<Uuid>, raw_request: serde_json::Value) -> ResponseStacked {
        let request: PlayerRequest<<S::M as SlotMath>::Input> = match serde_json::from_value(raw_request) {
            Ok(r) => r,
            Err(e) => {
                error!("{e}");
                return ResponseStacked {
                    id: None,
                    answer: Box::new(vec![Response::Error::<<S::M as SlotMath>::Special, <S::M as SlotMath>::Restore>(ErrorData {
                        message: "error request format!".to_string(),
                        code: ErrorType::UNKNOWN,
                    })]),
                    cache: false,
                };
            }
        };
        match request_id {
            None => {
                return ResponseStacked {
                    id: None,
                    answer: Box::new(vec![Response::Error::<<S::M as SlotMath>::Special, <S::M as SlotMath>::Restore>(ErrorData {
                        message: "Request id is null!".to_string(),
                        code: ErrorType::UNKNOWN,
                    })]),
                    cache: false,
                };
            }
            Some(id) => {
                if self.is_wrong_id(id) {
                    return ResponseStacked {
                        id: None,
                        answer: Box::new(vec![Response::Error::<<S::M as SlotMath>::Special, <S::M as SlotMath>::Restore>(ErrorData {
                            message: "Wrong request id!".to_string(),
                            code: ErrorType::UNKNOWN,
                        })]),
                        cache: false,
                    };
                }
            }
        }
        let r = match request {
            PlayerRequest::BetSpin(r) => self.on_spin(r).await,
            PlayerRequest::ReSpin => self.on_respin().await,
            PlayerRequest::FreeSpin => self.on_free_spin().await,
            PlayerRequest::Collect => self.on_collect(None).await,
            PlayerRequest::TournamentInfo => self.on_tournament_info().await,
            PlayerRequest::History(r) => self.on_history(r).await,
            PlayerRequest::Login(_) => Err(PlayerError::Account(acc_err(err_code::NOT_LOGGED_ON, line!(), file!()))),
        };
        match r {
            Ok(r) => r,
            Err(e) => {
                let data = self.on_error(e).await;
                ResponseStacked {
                    id: None,
                    answer: Box::new(vec![Response::Error::<<S::M as SlotMath>::Special, <S::M as SlotMath>::Restore>(data)]),
                    cache: false,
                }
            }
        }
    }

    async fn disconnect(&mut self, send_close: Option<OneShotSender<bool>>, game_session_id: Option<String>) {
        self.disconnect(send_close, game_session_id).await
    }

    fn on_balance(&mut self, balance: Decimal) -> Result<(), PlayerError> {
        self.on_balance(balance)
    }

    fn on_tournament_win(&mut self, win: TournamentWinData) {
        self.on_tournament_win(win)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateHolder {
    pub sessions: usize,
    pub clients: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OnlineHolder {
    pub count: usize,
}
