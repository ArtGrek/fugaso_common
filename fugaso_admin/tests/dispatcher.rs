use std::marker::PhantomData;

use async_trait::async_trait;
use chrono::Local;
use essential_async::channel::{self, OneShotSender};
use essential_core::{
    account_service::{AccountServiceFactory, ErrorType},
    digest::calc_hmac_sha256,
};
use fugaso_admin::dispatcher::{
    ClientRequest, Dispatcher, DispatcherContext, IDispacthercontext, OnlineHolder,
    ResponseStacked, SlotDispatcher, StateHolder, TournamentEventWin,
};
use fugaso_core::{
    admin::{StateLoader, TypedRepoFactory},
    protocol::{ErrorData, LoginRequest},
    proxy::{AuthData, JackpotProxyFactory, PromoServiceFactory, RetryServiceFactory},
};
use fugaso_data::{fugaso_action, fugaso_round, sequence_generator::IdGeneratorFactory};
use fugaso_math::math::SlotMath;
use log::error;
use sea_orm::prelude::Decimal;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub enum DispatcherSyncEvent<M: SlotMath> {
    Client(ClientRequest),
    Rand(M::Rand),
    Register {
        request: LoginRequest,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        sender: OneShotSender<Result<(ResponseStacked, String), ErrorData>>,
    },
    Balance(Decimal),
}

pub struct DispatcherSyncContext<
    M: SlotMath,
    FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory,
    FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory,
    S: StateLoader,
> {
    pub phatnom_m: PhantomData<M>,
    pub phantom_fa: PhantomData<FA>,
    pub phantom_fp: PhantomData<FP>,
    pub phantom_s: PhantomData<S>,
    sender: channel::UnboundedSender<DispatcherSyncEvent<M>>,
}

impl<
        M: SlotMath + Send + Sync + 'static,
        FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory + Send + Sync + 'static,
        FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory + Send + Sync + 'static,
        S: StateLoader + Send + Sync + 'static,
    > DispatcherSyncContext<M, FA, FP, S>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
    <M as SlotMath>::PlayFSM: Send + Sync,
    <M as SlotMath>::Calculator: Send + Sync,
    <M as SlotMath>::V: Send + Sync,
    <M as SlotMath>::Input: Send + Sync,
    <M as SlotMath>::Rand: Send + Sync,
{
    pub async fn new(mut dispatcher: SlotDispatcher<M, FA, FP, S>) -> Self {
        let (s, mut r) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(e) = r.recv().await {
                match e {
                    DispatcherSyncEvent::Client(req) => {
                        let response = dispatcher.handle(req.0, req.1).await;
                        req.2.send(response, file!(), line!());
                    }
                    DispatcherSyncEvent::Rand(r) => {
                        dispatcher.admin.math.set_rand(r);
                    }
                    DispatcherSyncEvent::Register {
                        request,
                        ip_address_list,
                        sender,
                        user_agent
                    } => {
                        let reponse =
                            Self::login_join(&mut dispatcher, request, ip_address_list, user_agent).await;
                        sender.send(reponse, file!(), line!());
                    }
                    DispatcherSyncEvent::Balance(b) => {
                        if let Err(e) = dispatcher.on_balance(b) {
                            error!("{e:?}");
                        };
                    }
                }
            }
        });
        Self {
            phatnom_m: PhantomData,
            phantom_fa: PhantomData,
            phantom_fp: PhantomData,
            phantom_s: PhantomData,
            sender: channel::UnboundedSender::new(s),
        }
    }

    async fn login_join(
        dispatcher: &mut SlotDispatcher<M, FA, FP, S>,
        request: LoginRequest,
        ip_address_list: Option<String>,
        user_agent: Option<String>
    ) -> Result<(ResponseStacked, String), ErrorData> {
        let (id, _)= dispatcher
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
                None
            )
            .await?;
        let response = dispatcher.join(id, request.country, None).await?;
        let now = Local::now();
        let token = calc_hmac_sha256(
            DispatcherContext::SECRET_KEY,
            &format!("{}:{}", id, now.timestamp_millis()),
        )?;
        Ok((response, token))
    }

    pub fn set_random(&self, r: <M as SlotMath>::Rand) {
        self.sender
            .send(DispatcherSyncEvent::Rand(r), file!(), line!());
    }

    pub fn set_balance(&self, balance: Decimal) {
        self.sender
            .send(DispatcherSyncEvent::Balance(balance), file!(), line!());
    }
}

#[async_trait]
impl<
        M: SlotMath + Send + Sync,
        FA: IdGeneratorFactory + PromoServiceFactory + TypedRepoFactory + Send + Sync,
        FP: JackpotProxyFactory + AccountServiceFactory + RetryServiceFactory + Send + Sync,
        S: StateLoader + Send + Sync,
    > IDispacthercontext for DispatcherSyncContext<M, FA, FP, S>
where
    <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
    <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
    <M as SlotMath>::PlayFSM: Send + Sync,
    <M as SlotMath>::Calculator: Send + Sync,
    <M as SlotMath>::Rand: Send + Sync,
{
    async fn register(
        &self,
        _dispatcher: Box<dyn Dispatcher + Send + Sync>,
        request: LoginRequest,
        ip_address_list: Option<String>,
        user_agent: Option<String>,
        _round_actions: Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>,
    ) -> Result<(ResponseStacked, String), ErrorData> {
        let (s, r) = oneshot::channel();
        self.sender.send(
            DispatcherSyncEvent::Register {
                request,
                ip_address_list,
                sender: OneShotSender::new(s),
                user_agent
            },
            file!(),
            line!(),
        );
        match r.await {
            Ok(r) => r,
            Err(_) => {
                error!("error answer holder");
                Err(ErrorData {
                    message: "Unknown error".to_string(),
                    code: ErrorType::UNKNOWN,
                })
            }
        }
    }

    async fn send(
        &self,
        req: (String, Option<Uuid>, serde_json::Value),
    ) -> Result<ResponseStacked, ErrorData> {
        let (s, r) = oneshot::channel();
        self.sender.send(
            DispatcherSyncEvent::Client(ClientRequest(req.1, req.2, OneShotSender::new(s))),
            file!(),
            line!(),
        );
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
        todo!()
    }

    async fn state(&self) -> Result<StateHolder, ErrorData> {
        todo!()
    }

    async fn ping(&self, _token: String) {
        todo!()
    }

    fn tournament_win(&self, _event: TournamentEventWin) {
        todo!()
    }
}
