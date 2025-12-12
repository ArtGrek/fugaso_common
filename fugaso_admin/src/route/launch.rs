use std::{collections::HashMap, marker::PhantomData};

use essential_core::account_service::ProxyAlias;
use essential_rand::random::RandomGenerator;
use lazy_static::lazy_static;
use log::error;
use reqwest::Url;
use salvo::{
    handler,
    http::{header::CONTENT_TYPE, HeaderValue},
    writing::Json,
    Depot, Request, Response, Router,
};
use serde::{Deserialize, Serialize};

use crate::err_on;
use crate::route::error::ErrData;
use crate::{config::ServerConfig, dispatcher::IDispacthercontext};

use super::options::options_handle;
use super::{client, error::LaunchError};
use maplit::hashmap;

pub const END_ROOT: &str = "launch";
pub const END_RERUN: &str = "rerun/{round_id}";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub games_dir: String,
    pub games_dir_no_jack: String,
    pub service_legacy: String,
    pub games_domain: Option<String>,
    pub service_name: Option<String>,
    pub curacao_on: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchData {
    pub jackpot: Option<bool>,
    pub simplex: bool,
    pub service: Option<String>,
    pub curacao: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArg {
    pub user_name: Option<String>,
    pub password: Option<String>,
    pub session_id: Option<String>,
    pub mode: ProxyAlias,
    pub operator_id: Option<i64>,
    pub game_name: String,
    pub close_url: Option<String>,
    pub responsible_game: Option<bool>,
    pub reality_check_elapsed: Option<i32>,
    pub reality_check_interval: Option<i32>,
    pub history_url: Option<String>,
    #[serde(default)]
    pub lobby: bool,
    pub jackpot: Option<bool>,
    pub language: Option<String>,
    pub lang: Option<String>,
    pub country: Option<String>,
    pub social: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LaunchResponse {
    pub url: String,
}

lazy_static! {
    pub static ref LEGACY: HashMap<&'static str, LaunchData> = hashmap! {
    "olympia" => LaunchData{simplex: true, ..Default::default()},
    "jewelsea" => LaunchData{simplex: true, ..Default::default()},
    "magicdestiny" => Default::default(),
    "sunset" => LaunchData{simplex: true, ..Default::default()},
    "mrtoxicus" => LaunchData{simplex: true, ..Default::default()},
    "yakuza" => LaunchData{simplex: true, ..Default::default()},
    "carousel" => Default::default(),
    "saharasdreams" => Default::default(),
    "robbiejones" => Default::default(),
    "doublecash" => Default::default(),
    "shakeit" => Default::default(),
    "fruitsofneon" => Default::default(),
    "trumpit" => LaunchData{simplex: true, ..Default::default()},
    "lagertha" => Default::default(),
    "cosanostra" => Default::default(),
    "crazybot" => LaunchData{simplex: true, ..Default::default()},
    "knockout" => LaunchData{simplex: true, ..Default::default()},
    "thegiant" => Default::default(),
    "smokingdogs" => Default::default(),
    "grandsumo" => Default::default(),
    "megapowerheroes" => LaunchData{simplex: true, ..Default::default()},
    "bookoftattoo" => LaunchData{simplex: true, ..Default::default()},
    "numberone" => Default::default(),
    "neonblackjack" => Default::default(),
    "fromchinawithlove" => Default::default(),
    "fearthezombies" => LaunchData{simplex: true, ..Default::default()},
    "plaguesofegypt" => LaunchData{simplex: true, ..Default::default()},
    "trumpitblackjack" => Default::default(),
    "graffiti" => Default::default(),
    "neonblackjackmobile" => Default::default(),
    "trumpitblackjackmobile" => Default::default(),
    "neonblackjackonedeck" => Default::default(),
    "trumpitblackjackonedeck" => Default::default(),
    "forro" => Default::default(),
    "neonroulette" => Default::default(),
    "cheerfulfarmer" => Default::default(),
    "grillking" => Default::default(),
    "forestant" => Default::default(),
    "gemstoneofaztec" => Default::default(),
    "lapland" => Default::default(),
    "maniachouse" => Default::default(),
    "seaunderwaterclub" => Default::default(),
    "treasureofshaman" => Default::default(),
    "gatesofhell" => Default::default(),
    "sweetparadise" => Default::default(),
    "goblinsland" => Default::default(),
    "spacebattle" => Default::default(),
    "horrorcastle" => Default::default(),
    "mummyRemoved" => Default::default(),
    "goldenshot" => Default::default(),
    "luckyspineuroroulette" => Default::default(),
    "bravemongoose" => Default::default(),
    "revengeofcyborgs" => Default::default(),
    "nrgsound" => Default::default(),
    "powerofasia" => Default::default(),
    "evilgenotype" => Default::default(),
    "wildrodeo" => Default::default(),
    "warlocksbook" => Default::default(),
    "resident3d" => Default::default(),
    "trumpitdeluxe" => LaunchData{simplex: true, ..Default::default()},
    "stonedjoker" => Default::default(),
    "superhamster" => Default::default(),
    "stonedjoker5" => Default::default(),
    "deepbluesea" => Default::default(),
    "fugasoairlines" => Default::default(),
    "imhotepmanuscript" => Default::default(),
    "magicspinners" => Default::default(),
    "cleopatrasdiary" => Default::default(),
    "bookoftattoo2" => Default::default(),
    "spinjokerspin" => Default::default(),
    "divinecarnival" => Default::default(),
    "clashofgods" => Default::default(),
    "donslottione" => Default::default(),
    "trumpitdeluxeepicways" => Default::default(),
    "bookofparimatch" => LaunchData{jackpot: Some(false), ..Default::default()},
    "paristars" => LaunchData{jackpot: Some(false), ..Default::default()},
    "jokermatch" => LaunchData{jackpot: Some(false), ..Default::default()},
    "jokermatch5" => LaunchData{jackpot: Some(false), ..Default::default()},
    "paristars5" => LaunchData{jackpot: Some(false), ..Default::default()},
    "royalmatch" => LaunchData{jackpot: Some(false), ..Default::default()},
    "kingofparimatch" => LaunchData{jackpot: Some(false), ..Default::default()},
    "spintysonspin" => Default::default(),
    "ladyofparimatch" => Default::default(),
    "donparimatch" => Default::default(),
    "riseofparimatch" => Default::default(),
    "romancev" => Default::default(),
    "themummyepicways" => Default::default(),
    "mummywinhunters" => LaunchData{simplex: true, ..Default::default()},
    "fortunecircus" => LaunchData{simplex: true, ..Default::default()},
    "theswordthemagic" => Default::default(),
    "bookofanime" => LaunchData{simplex: true, ..Default::default()},
    "jewelseapirateriches" => LaunchData{simplex: true, ..Default::default()},
    "santasjinglewheel" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "wheelofparimatch" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "fatmamaswheel" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "diamondblitz40" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "hitthediamond" => LaunchData{simplex: true, ..Default::default()},
    "sugardrop" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "kingofthering" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "diamondblitz100" => LaunchData{simplex: true, ..Default::default()},
    "lilsanta" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "sugardropxmas" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "lilsantabonusbuy" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "infernodiamonds" => LaunchData{simplex: true, ..Default::default()},
    "tropicalgold" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "intothejungle" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "mostbetinferno" => LaunchData{simplex: true, ..Default::default()},
    "infernodiamonds100" => LaunchData{simplex: true, ..Default::default()},
    "intothejunglebonusbuy" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "popanddrop" => LaunchData{simplex: true, jackpot: Some(false), ..Default::default()},
    "infernodevil100" => LaunchData{simplex: true, ..Default::default()},
    "sugarparadise" => LaunchData{simplex: true, ..Default::default()},
    "powerwildzfs" => LaunchData{simplex: true, curacao: true, ..Default::default()},
    "magnifyman" => LaunchData{service: Some("magnify-admin/player".to_string()), ..Default::default()},
    "pariman" => LaunchData{service: Some("pariman-admin/player".to_string()), ..Default::default()},
    "vulkanvegasman" => LaunchData{service: Some("vulkan-admin/player".to_string()), ..Default::default()},
    "brxmoneyman" => LaunchData{service: Some("brxmoney-admin/player".to_string()), ..Default::default()},
    "1winspacegirl" => LaunchData{service: Some("1winspacegirl-admin/player".to_string()), ..Default::default()},
    };
}

pub fn create<D: IDispacthercontext + Send + Sync + 'static>() -> Router {
    Router::with_path(END_ROOT)
        .post(Launch::<D> {
            phantom: PhantomData,
        })
        .get(Launch::<D> {
            phantom: PhantomData,
        })
        .options(options_handle)
}

pub fn create_rerun<D: IDispacthercontext + Send + Sync + 'static>() -> Router {
    Router::with_path(END_RERUN)
        .post(Rerun::<D> {
            phantom: PhantomData,
        })
        .get(Rerun::<D> {
            phantom: PhantomData,
        })
        .options(options_handle)
}

pub struct Launch<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Launch<D> {
    pub async fn handle(req: &mut Request, res: &mut Response, depot: &mut Depot) -> Result<(), LaunchError> {
        let arg = req.parse_queries::<LaunchArg>()?;
        let cfg = depot.obtain::<ServerConfig<D>>().map_err(|_| LaunchError::Server(err_on!("context error!")))?;

        launch(arg, client::END_ROOT, cfg, req, res).await
    }
}

pub struct Rerun<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Rerun<D> {
    pub async fn handle(req: &mut Request, res: &mut Response, depot: &mut Depot) -> Result<(), LaunchError> {
        let cfg = depot.obtain::<ServerConfig<D>>().map_err(|_| LaunchError::Server(err_on!("context error!")))?;
        let round_id = req.try_param::<i64>("round_id")?;

        let mut rounds = cfg.round_repo.find_round_finished(round_id).await.map_err(|e| LaunchError::Server(err_on!(e)))?;
        let arg = if let Some(mut p) = rounds.pop() {
            p.1.sort_by_key(|a| a.id);
            let game_id = p.0.game_id.ok_or_else(|| LaunchError::Server(err_on!(format!("game id is none on {}!", p.0.id))))?;
            let game_name = cfg
                .game_service
                .get_game_by_id(game_id)
                .await?
                .and_then(|g| g.game_name)
                .ok_or_else(|| LaunchError::Server(err_on!(format!("game name is none on {:?}!", game_id))))?;
            LaunchArg {
                game_name: game_name,
                mode: ProxyAlias::Demo,
                user_name: p.0.user_id.map(|u| u.to_string()),
                password: Some("000000".to_string()),
                session_id: p.0.user_id.map(|u| u.to_string()),
                ..Default::default()
            }
        } else {
            return Err(LaunchError::Game(err_on!(format!("round {round_id} is none!"))));
        };

        launch(arg, &format!("{}/{round_id}", client::END_REPLAY), cfg, req, res).await
    }
}

pub async fn launch<D: IDispacthercontext + Send + Sync + 'static>(
    arg: LaunchArg,
    service_path: &str,
    cfg: &ServerConfig<D>,
    req: &mut Request,
    res: &mut Response,
) -> Result<(), LaunchError> {
    let launch_repo = cfg.launch_repo.clone();
    let infos = cfg.launch_cache.try_get_with(0, async move { launch_repo.find_all(false).await }).await.inspect_err(|e| error!("{e:?}")).unwrap_or(vec![]);
    let server_name = if infos.len() > 0 {
        if infos.len() > 1 {
            let mut rand = RandomGenerator::new();
            let i = rand.random(0, infos.len());
            infos[i].host_name.clone()
        } else {
            infos[0].host_name.clone()
        }
    } else {
        req.header::<String>("x-forwarded-host").ok_or_else(|| LaunchError::Header(err_on!("host is none!")))?
    };

    let context_path = req.header::<String>("x-forwarded-prefix").ok_or_else(|| LaunchError::Header(err_on!("forward prefix is none!")))?;
    let jack_off = req.header::<&str>("X-Accumulate") == Some("No");

    let context = format!("https://{server_name}");
    let folder = if jack_off {
        cfg.launch_cfg.games_dir_no_jack.as_str()
    } else {
        cfg.launch_cfg.games_dir.as_str()
    };
    let context_on = if let Some(d) = cfg.launch_cfg.games_domain.as_ref() {
        d.clone()
    } else {
        context.clone()
    };
    let game = cfg.game_service.get_game(&arg.game_name).await?.ok_or_else(|| LaunchError::Game(err_on!(format!("game: {} not found!", arg.game_name))))?;

    let mut url = Url::parse_with_params(&format!("{context_on}/{folder}/{}/index.html", arg.game_name), &[("gameName", arg.game_name.as_str())])
        .map_err(|e| LaunchError::Url(err_on!(e)))?;
    append_key_value(&mut url, "userName", arg.user_name);
    append_key_value(&mut url, "password", arg.password);
    append_key_value(&mut url, "sessionId", arg.session_id);

    append_value(&mut url, "responsibleGame", arg.responsible_game);
    append_value(&mut url, "operatorId", arg.operator_id);
    append_value(&mut url, "realityCheckElapsed", arg.reality_check_elapsed);
    append_value(&mut url, "realityCheckInterval", arg.reality_check_interval);
    append_value(&mut url, "historyUrl", arg.history_url);

    append_value(&mut url, "language", arg.language);
    append_value(&mut url, "lang", arg.lang);
    append_value(&mut url, "country", arg.country);

    let launch_default = LaunchData {
        service: Some(format!("{context_path}/{service_path}")),
        curacao: true,
        ..Default::default()
    };
    let launch_on = LEGACY.get(arg.game_name.as_str()).unwrap_or(&launch_default);
    append_value(&mut url, "jackpot", launch_on.jackpot.or(arg.jackpot));

    let mut server = format!("{server_name}/{}/duplex", cfg.launch_cfg.service_legacy);
    if launch_on.simplex {
        server = format!("{server_name}/{}/{}", cfg.launch_cfg.service_legacy, client::END_ROOT);
    }
    if let Some(s) = launch_on.service.as_ref() {
        server = format!("{server_name}/{}", s);
    }
    if cfg.launch_cfg.curacao_on && launch_on.curacao {
        url.query_pairs_mut().append_pair("curacao", &true.to_string());
    }

    url.query_pairs_mut().append_pair("mode", &arg.mode.to_string());
    url.query_pairs_mut().append_pair("hostUrl", &server);
    url.query_pairs_mut().append_pair("lobby", &arg.lobby.to_string());
    url.query_pairs_mut().append_pair("closeUrl", &arg.close_url.unwrap_or(format!("{context}/{folder}/lobby/index.html")));
    url.query_pairs_mut().append_pair("autoPlay", &false.to_string());
    append_value(&mut url, "tourTheme", game.tour_theme);
    append_value(&mut url, "social", arg.social);

    res.render(Json(LaunchResponse {
        url: url.to_string(),
    }));
    res.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    Ok(())
}

fn append_value<T: ToString>(url: &mut Url, key: &str, value: Option<T>) {
    if let Some(c) = value {
        url.query_pairs_mut().append_pair(key, &c.to_string());
    }
}

fn append_key_value(url: &mut Url, key: &str, value: Option<String>) {
    if let Some(c) = value {
        url.query_pairs_mut().append_pair(key, &c);
    } else {
        url.query_pairs_mut().append_key_only(key);
    }
}
