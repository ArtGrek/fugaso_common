use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use config::FileFormat;
use essential_core::account_service::{AccountService, AccountServiceFactory, DeferredFactory, ProxyAlias};
use essential_core::config::{ApplicationConfig, Arg};
use essential_core::error::message::GAME_MATH_ERROR;
use essential_core::error::ServerError;
use essential_core::jackpot_service::{JackpotEmptyAwardProxy, JackpotEmptyProxy, JackpotProxy};
use essential_core::{account_service, err_on};
use essential_data::repo::TypedRepository;
use essential_data::{account_account, account_entry, account_transaction};
use essential_test::database_configuration::{insert_euro_currency, setup_schema};
use fugaso_math_ed6::math::ThunderExpressMath;
use log::info;

use moka::future::Cache;
use salvo::conn::TcpListener;
use salvo::cors::Cors;
use salvo::server::ServerHandle;
use salvo::{affix_state, Listener, Router, Server};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use serde::{Deserialize, Serialize};
use tokio::signal;
use tokio::sync::oneshot;

use fugaso_core::admin::{
    AdminConfig, BetConfigurator, DemoStoreService, RealStoreService, SlotAdmin, StateLoader, StepSettings, StoreService, StoreServiceFactory, TypedRepoFactory,
};
use fugaso_core::proxy::{
    GameService, IpServiceConfig, JackpotProxyFactory, PromoService, PromoServiceFactory, ProxyConfig, RetryService, RetryServiceFactory, SlotProxy, UserInformationService,
};
use fugaso_core::tournament::{TournamentConfig, TournamentGainService, TournamentHolder};
use fugaso_data::repo::{
    GameRepository, LaunchInfoRepository, PercentRepository, PromoAccountRepository, PromoStatsRepository, PromoTranRepository, RoundRepository, TournamentGainRepository,
};
use fugaso_data::sequence_generator::{DemoIdGenerator, FugasoIdGenerator, IdGenerator, IdGeneratorFactory};
use fugaso_data::{common_round, fugaso_action, fugaso_game, fugaso_round, launch_info, promo_account, promo_stats, promo_transaction, tournament_gain};
use fugaso_math::math::SlotMath;

use crate::database;
use crate::dispatcher::{Dispatcher, DispatcherConfig, DispatcherContext, IDispacthercontext, JackpotDispatcher, SlotDispatcher};
use crate::logger::setup_logger;
use crate::manager::TournamentManager;
use crate::route;
use crate::route::launch::LaunchConfig;
#[cfg(not(any(feature = "server_116_202_218_41_playtech", feature = "server_159_69_70_159_playtech")))]
use fugaso_core::admin::SuccessStateLoader as AdminStateLoader;
use fugaso_test::database_configuration::setup_schema_fugaso_game;
#[cfg(feature = "redis")]
use {essential_data::repo::DeferredRepository, mobc_redis::mobc::Pool, mobc_redis::RedisConnectionManager};

pub struct ServerConfig<D: IDispacthercontext + Send + Sync> {
    pub p: ApplicationConfig,
    #[cfg(feature = "redis")]
    pub red_pool: Pool<RedisConnectionManager>,

    pub round_repo: Arc<RoundRepository>,
    pub gain_repo: Arc<TournamentGainRepository>,
    pub promo_acc_repo: Arc<PromoAccountRepository>,
    pub promo_stats_repo: Arc<PromoStatsRepository>,
    pub promo_tran_repo: Arc<PromoTranRepository>,
    pub launch_repo: Arc<LaunchInfoRepository>,

    pub percent_repo: Arc<PercentRepository>,
    pub table_id_gen: Arc<FugasoIdGenerator>,
    pub demo_id_gen: Arc<DemoIdGenerator>,
    pub bet_configurator: Arc<BetConfigurator>,

    pub jackpot_dispatcher: Arc<JackpotDispatcher>,
    pub game_service: Arc<GameService>,
    pub dispatcher_context: Arc<D>,
    pub tour_manager: Arc<TournamentManager>,
    pub tour_holder: Arc<TournamentHolder>,
    pub gain_service: Arc<TournamentGainService>,
    pub info_service: Arc<UserInformationService>,
    pub admin_config: Arc<AdminConfig>,
    pub proxy_config: ProxyConfig,
    pub launch_cfg: LaunchConfig,
    pub game_config: bool,
    pub name: String,
    pub launch_cache: Cache<i32, Vec<launch_info::Model>>,
}

pub const LAUNCH_CACHE_SEC: u64 = 20 * 60;

impl<D: IDispacthercontext + Send + Sync> Clone for ServerConfig<D> {
    fn clone(&self) -> ServerConfig<D> {
        Self {
            admin_config: self.admin_config.clone(),
            p: self.p.clone(),
            round_repo: self.round_repo.clone(),
            gain_repo: self.gain_repo.clone(),
            promo_acc_repo: self.promo_acc_repo.clone(),
            promo_stats_repo: self.promo_stats_repo.clone(),
            promo_tran_repo: self.promo_tran_repo.clone(),
            percent_repo: self.percent_repo.clone(),
            launch_repo: self.launch_repo.clone(),
            table_id_gen: self.table_id_gen.clone(),
            demo_id_gen: self.demo_id_gen.clone(),
            bet_configurator: self.bet_configurator.clone(),
            jackpot_dispatcher: self.jackpot_dispatcher.clone(),
            game_service: self.game_service.clone(),
            dispatcher_context: self.dispatcher_context.clone(),
            tour_manager: self.tour_manager.clone(),
            tour_holder: self.tour_holder.clone(),
            gain_service: self.gain_service.clone(),
            info_service: self.info_service.clone(),
            proxy_config: self.proxy_config.clone(),
            game_config: self.game_config,
            launch_cfg: self.launch_cfg.clone(),
            name: self.name.clone(),
            launch_cache: self.launch_cache.clone(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RedisConfig {
    pub name: String,
    pub ip: String,
    pub password: String,
}

impl RedisConfig {
    pub fn url(&self) -> String {
        format!("redis://{}:{}@{}", self.name, self.password, self.ip)
    }
}

#[derive(Debug, Default)]
pub struct ServerArg {
    pub name: String,
    pub pool: DatabaseConnection,
    pub tour_config: TournamentConfig,
    pub admin_config: AdminConfig,
    pub ip_service_config: IpServiceConfig,
    pub redis_config: RedisConfig,
    pub dispatcher_config: DispatcherConfig,
    pub proxy_config: ProxyConfig,
    pub game_config: bool,
    pub launch_cfg: LaunchConfig,
}

impl ServerConfig<DispatcherContext> {
    pub async fn new(arg: ServerArg) -> Result<Self, ServerError> {
        let dispatcher_context = Arc::new(DispatcherContext::new(&arg.dispatcher_config));
        Self::custom(arg, dispatcher_context).await
    }
}

impl<D: IDispacthercontext + Send + Sync + 'static> ServerConfig<D> {
    pub async fn custom(arg: ServerArg, dispatcher_context: Arc<D>) -> Result<Self, ServerError> {
        setup_logger(&arg.name);
        #[cfg(feature = "redis")]
        let red_pool = Self::create_redis_pool(arg.redis_config).await?;

        let base = ApplicationConfig::new(Arg {
            pool: arg.pool,
            setup_logger: false,
        })
        .await?;

        let round_repo = Arc::new(RoundRepository {
            conn: Arc::clone(&base.pool),
        });
        let percent_repo = Arc::new(PercentRepository {
            conn: Arc::clone(&base.pool),
        });
        let game_repo = Arc::new(GameRepository {
            conn: Arc::clone(&base.pool),
        });

        let table_id_gen = Arc::new(FugasoIdGenerator {
            common_round_id_gen: Box::new(common_round::create_sequence(Arc::clone(&base.pool))),
            round_id_gen: Box::new(fugaso_round::create_sequence(Arc::clone(&base.pool))),
            action_id_gen: Box::new(fugaso_action::create_sequence(Arc::clone(&base.pool))),
            gain_id_gen: Box::new(tournament_gain::create_sequence(Arc::clone(&base.pool))),
            promo_account_id_gen: Box::new(promo_account::create_sequence(Arc::clone(&base.pool))),
            promo_stats_id_gen: Box::new(promo_stats::create_sequence(Arc::clone(&base.pool))),
            promo_tran_id_gen: Box::new(promo_transaction::create_sequence(Arc::clone(&base.pool))),
        });
        let demo_id_gen = Arc::new(DemoIdGenerator::new());

        let bet_configurator = Arc::new(BetConfigurator::new(Arc::clone(&base.currency_repo), Arc::clone(&base.exchange_repo), fugaso_config::BETS).await?);

        let jackpot_dispatcher = Arc::new(JackpotDispatcher::new(Arc::clone(&base.jackpot_repo), Duration::from_secs(5)));
        let game_service = Arc::new(GameService::new(game_repo));

        let tour_holder = Arc::new(TournamentHolder::new(Arc::new(arg.tour_config), arg.name.clone()).await);
        let admin_config = Arc::new(arg.admin_config);
        let proxy_config = arg.proxy_config;
        let gain_repo = Arc::new(TournamentGainRepository {
            conn: Arc::clone(&base.pool),
        });
        let promo_acc_repo = Arc::new(PromoAccountRepository {
            conn: Arc::clone(&base.pool),
        });
        let promo_stats_repo = Arc::new(PromoStatsRepository {
            conn: Arc::clone(&base.pool),
        });
        let promo_tran_repo = Arc::new(PromoTranRepository {
            conn: Arc::clone(&base.pool),
        });
        let launch_repo = Arc::new(LaunchInfoRepository {
            conn: Arc::clone(&base.pool),
        });
        let launch_cache = Cache::builder()
            // Max 10,000 entries
            .max_capacity(5)
            // Time to live (TTL): 30 minutes
            .time_to_live(Duration::from_secs(LAUNCH_CACHE_SEC))
            // Create the cache.
            .build();
        let gain_service = Arc::new(TournamentGainService::new(Arc::clone(&tour_holder)));
        let tour_manager = Arc::new(TournamentManager::new(
            Arc::clone(&base.base_repo),
            Arc::clone(&base.user_repo),
            Arc::clone(&gain_service),
            Arc::clone(&gain_repo),
            Arc::clone(&table_id_gen),
            Arc::clone(&tour_holder.config),
        ));
        let info_service = Arc::new(UserInformationService::new(Arc::clone(&base.base_repo), Arc::clone(&base.user_info_repo), arg.ip_service_config));
        Ok(Self {
            p: base,
            #[cfg(feature = "redis")]
            red_pool,
            round_repo,
            percent_repo,
            promo_acc_repo,
            promo_stats_repo,
            promo_tran_repo,
            launch_repo,
            table_id_gen,
            demo_id_gen,
            bet_configurator,
            jackpot_dispatcher,
            game_service,
            dispatcher_context,
            tour_holder,
            admin_config,
            gain_service,
            tour_manager,
            info_service,
            gain_repo,
            proxy_config,
            game_config: arg.game_config,
            launch_cfg: arg.launch_cfg,
            name: arg.name,
            launch_cache,
        })
    }

    pub fn create_admin<M: SlotMath, S: StateLoader>(&self, math: M, state_loader: S) -> SlotAdmin<M, Self, S>
    where
        <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
        <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
    {
        SlotAdmin::new(
            Arc::new(self.clone()),
            Arc::clone(&self.p.base_repo),
            Arc::clone(&self.percent_repo),
            Arc::clone(&self.p.user_settings_repo),
            Arc::clone(&self.table_id_gen),
            Arc::clone(&self.round_repo),
            Arc::clone(&self.bet_configurator),
            Arc::clone(&self.admin_config),
            math,
            state_loader,
        )
    }

    pub async fn create_proxy<M: SlotMath>(&self, game: fugaso_game::Model) -> Result<SlotProxy<M, Self>, ServerError>
    where
        <M as SlotMath>::Special: Serialize + Sync + Send,
    {
        SlotProxy::new(
            Arc::new(self.clone()),
            Box::new(JackpotEmptyAwardProxy {}),
            Arc::clone(&self.game_service),
            Arc::clone(&self.p.base_repo),
            Arc::clone(&self.p.currency_repo),
            Arc::clone(&self.p.user_attr_repo),
            Arc::clone(&self.gain_repo),
            Arc::clone(&self.tour_holder),
            Arc::clone(&self.gain_service),
            Arc::clone(&self.info_service),
            self.proxy_config.clone(),
            game,
            StepSettings::default(),
        )
        .await
    }

    #[cfg(not(any(feature = "server_116_202_218_41_playtech", feature = "server_159_69_70_159_playtech")))]
    pub async fn create_slot_dispatcher<M: SlotMath + Send + Sync>(&self, math: M, game: fugaso_game::Model) -> Result<SlotDispatcher<M, Self, Self, AdminStateLoader>, ServerError>
    where
        <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
        <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
        <M as SlotMath>::Input: Serialize + Sync + Send + 'static,
        <M as SlotMath>::V: Sync + Send + 'static,
        <M as SlotMath>::PlayFSM: Sync + Send + 'static,
        <M as SlotMath>::Calculator: Sync + Send + 'static,
    {
        Ok(SlotDispatcher::new(
            Arc::clone(&self.jackpot_dispatcher),
            self.create_proxy::<M>(game).await?,
            self.create_admin(
                math,
                AdminStateLoader {
                    round_repo: Arc::clone(&self.round_repo),
                },
            ),
        ))
    }

    pub async fn create_game_dispatcher<M: SlotMath + Send + Sync + 'static>(
        &self,
        math: M,
        game: fugaso_game::Model,
        _replay: bool,
    ) -> Result<Box<dyn Dispatcher + Sync + Send>, ServerError>
    where
        <M as SlotMath>::Special: Serialize + Sync + Send + 'static,
        <M as SlotMath>::Restore: Serialize + Sync + Send + 'static,
        <M as SlotMath>::Input: Serialize + Sync + Send + 'static,
        <M as SlotMath>::V: Sync + Send + 'static,
        <M as SlotMath>::PlayFSM: Sync + Send + 'static,
        <M as SlotMath>::Calculator: Sync + Send + 'static,
    {
        Ok(Box::new(self.create_slot_dispatcher(math, game).await?))
    }

    pub async fn create_dispatcher(&self, game_name: &str, replay: bool) -> Result<Box<dyn Dispatcher + Sync + Send>, ServerError> {
        let g = self.game_service.get_game(game_name).await.map_err(|e| err_on!(e))?.ok_or_else(|| err_on!(GAME_MATH_ERROR))?;

        let (config, reels_cfg) = (None, None);
        if g.math_class == stringify!(ThunderExpressMath) {
            self.create_game_dispatcher(ThunderExpressMath::new(config, reels_cfg)?, g, replay).await
        } else {
            Err(err_on!("game is not supported!"))
        }
    }
}

#[async_trait]
impl<D: IDispacthercontext + Send + Sync> TypedRepoFactory for ServerConfig<D> {
    #[cfg(not(feature = "redis"))]
    async fn create_repo(
        &self,
        _user_id: i64,
    ) -> Result<
        Arc<
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
        ServerError,
    > {
        todo!()
    }
}

impl<D: IDispacthercontext + Send + Sync> JackpotProxyFactory for ServerConfig<D> {
    fn create_jackpot_proxy(&self, _math_class: &str) -> Box<dyn JackpotProxy + Send + Sync> {
        Box::new(JackpotEmptyProxy {})
    }
}

#[async_trait]
impl<D: IDispacthercontext + Send + Sync> PromoServiceFactory for ServerConfig<D> {
    async fn create_real_promo_service(&self, _user_id: i64, _game_id: i64) -> Result<Box<dyn PromoService + Send + Sync>, ServerError> {
        todo!()
    }
}

impl<D: IDispacthercontext + Send + Sync> StoreServiceFactory for ServerConfig<D> {
    fn create_store_service(&self, demo: bool) -> Box<dyn StoreService + Send + Sync> {
        if demo {
            Box::new(DemoStoreService)
        } else {
            Box::new(RealStoreService)
        }
    }
}

impl<D: IDispacthercontext + Send + Sync> IdGeneratorFactory for ServerConfig<D> {
    fn create(&self, _demo: bool) -> Arc<dyn IdGenerator + Send + Sync> {
        Arc::clone(&self.demo_id_gen) as Arc<dyn IdGenerator + Send + Sync>
    }
}

#[async_trait]
impl<D: IDispacthercontext + Send + Sync> account_service::TypedRepoFactory for ServerConfig<D> {
    #[cfg(not(feature = "redis"))]
    async fn create_repo(
        &self,
        _user_id: i64,
    ) -> Result<
        Arc<
            dyn TypedRepository<
                    account_transaction::ActiveModel,
                    account_entry::ActiveModel,
                    account_transaction::ActiveModel,
                    account_transaction::ActiveModel,
                    account_account::ActiveModel,
                    account_account::ActiveModel,
                > + Send
                + Sync,
        >,
        ServerError,
    > {
        let repo = Arc::clone(&self.p.base_repo);
        Ok(repo)
    }
}

#[async_trait]
impl<D: IDispacthercontext + Send + Sync + 'static> AccountServiceFactory for ServerConfig<D> {
    async fn create_account_service(&self, mode: &ProxyAlias) -> Result<Box<dyn AccountService + Send + Sync>, ServerError> {
        match mode {
            _ => Ok(Box::new(self.p.create_demo_account_service())),
        }
    }
}

impl<D: IDispacthercontext + Send + Sync> RetryServiceFactory for ServerConfig<D> {
    fn create_retry_service(&self, _factory: DeferredFactory) -> Box<dyn RetryService + Send + Sync> {
        todo!()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    pub host: String,
    pub port: i32,
    pub path: String,
    pub cache: bool,
}

impl HttpConfig {
    pub fn url_handle(&self) -> String {
        format!("http://{}:{}/{}/{}", self.host, self.port, route::client::END_ROOT, route::client::END_HANDLE)
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSettings {
    pub name: String,
    pub server_config: HttpConfig,
    pub database_connection: String,
    pub tour_config: TournamentConfig,
    pub admin_config: AdminConfig,
    pub ip_service_config: IpServiceConfig,
    pub redis_config: RedisConfig,
    pub dispatcher_config: DispatcherConfig,
    pub proxy_config: ProxyConfig,
    #[serde(default)]
    pub game_config: bool,
    pub launch_config: LaunchConfig,
}

impl ApplicationSettings {
    pub fn new() -> Result<Self, config::ConfigError> {
        let mut default = config::Config::builder();
        default = default.add_source(config::File::from_str(CONFIG_DEFAULT, FileFormat::Json)).add_source(config::Environment::with_prefix("CONTAINER"));
        default.build()?.try_deserialize()
    }
}

const CONFIG_DEFAULT: &str = include_str!("resources/application.json");

pub async fn run_server() {
    let app_settings: ApplicationSettings = ApplicationSettings::new().expect("error read application settings!");

    let mut opt = ConnectOptions::new("sqlite::memory:".to_owned());
    opt.max_connections(2).min_connections(2).connect_timeout(Duration::from_secs(8)).idle_timeout(Duration::from_secs(8)).max_lifetime(Duration::from_secs(8)).sqlx_logging(true);
    let pool = Database::connect(opt).await.expect("error open connection");
    setup_schema(&pool).await;
    setup_schema_fugaso_game(&pool).await;
    insert_euro_currency(&pool).await;
    database::insert_games(&pool).await.expect("error insert games!");

    /*let mut opt = ConnectOptions::new(app_settings.database_connection);
    opt.max_connections(300)
        .min_connections(2)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(60 * 60))
        .max_lifetime(Duration::from_secs(24 * 60 * 60))
        .sqlx_logging(true);
    let pool = Database::connect(opt).await.expect("error open connection");*/

    let cfg = ServerConfig::new(ServerArg {
        pool,
        name: app_settings.name,
        tour_config: app_settings.tour_config,
        admin_config: app_settings.admin_config,
        ip_service_config: app_settings.ip_service_config,
        redis_config: app_settings.redis_config,
        dispatcher_config: app_settings.dispatcher_config,
        proxy_config: app_settings.proxy_config,
        game_config: app_settings.game_config,
        launch_cfg: app_settings.launch_config,
    })
    .await
    .expect("error create config");
    bootstrap(cfg, app_settings.server_config, None).await
}

pub async fn create_server<D: IDispacthercontext + Send + Sync + 'static>(server_config: ServerConfig<D>, http_config: HttpConfig, shutdown: oneshot::Receiver<()>)
where
    ServerConfig<D>: Clone,
{
    bootstrap(server_config, http_config, Some(shutdown)).await
}

#[cfg(not(feature = "server_116_202_218_41_supply"))]
fn create_route<D: IDispacthercontext + Send + Sync + 'static>(server_config: ServerConfig<D>, http_config: &HttpConfig) -> Router {
    let cors = Cors::permissive().into_handler();
    Router::with_hoop(cors)
        .hoop(affix_state::inject(server_config))
        .push(route::client::create::<D>(Some(&http_config.path)))
        .push(route::client::create_replay::<D>())
        .push(route::metrics::create::<D>())
        .push(route::actuator::create())
        .push(route::tournament::create::<D>())
        .push(route::health::create())
        .push(route::launch::create::<D>())
        .push(route::launch::create_rerun::<D>())
}

pub async fn bootstrap<D: IDispacthercontext + Send + Sync + 'static>(server_config: ServerConfig<D>, http_config: HttpConfig, shutdown: Option<oneshot::Receiver<()>>)
where
    ServerConfig<D>: Clone,
{
    let router = create_route::<D>(server_config, &http_config);

    let acceptor = TcpListener::new(http_config.address()).bind().await;
    let server = Server::new(acceptor);
    let handle = server.handle();
    if let Some(s) = shutdown {
        tokio::spawn(async move {
            if let Ok(_) = s.await {
                handle.stop_graceful(None);
            }
        });
    } else {
        tokio::spawn(listen_shutdown_signal(handle));
    };
    server.serve(router).await
}

async fn listen_shutdown_signal(handle: ServerHandle) {
    // Wait Shutdown Signal
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate()).expect("failed to install signal handler").recv().await;
    };

    #[cfg(windows)]
    let terminate = async {
        signal::windows::ctrl_c().expect("failed to install signal handler").recv().await;
    };

    tokio::select! {
        _ = ctrl_c => info!("ctrl_c signal received"),
        _ = terminate => info!("terminate signal received"),
    };
    // Graceful Shutdown Server
    handle.stop_graceful(Some(Duration::from_secs(30)));
}
