mod dispatcher;
mod integration;

use crate::integration::{assert_answer, create_collect, parse_game_data};
use dispatcher::DispatcherSyncContext;
use fugaso_admin::config::{create_server, ServerArg, ServerConfig};
use fugaso_admin::dispatcher::DispatcherContext;
use fugaso_admin::route::client::{AUTH_TOKEN, REQUEST_ID};
use fugaso_core::admin::SuccessStateLoader;
use fugaso_math::protocol::GameData;
use fugaso_math_ed7::config::mega_thunder as gconf;
use fugaso_math_ed7::math::MegaThunderMath;
use fugaso_math_ed7::rand::MockMegaThunderRand;
use integration::{create_connection, create_http_cfg};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Url};
use sea_orm::prelude::Decimal;
use serde_json::Value;
use std::collections::vec_deque::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::sync::oneshot;

const GAME_NAME: &str = "megathunder";
const GAME_FOLDER: &str = "mega_thunder";
type Math = MegaThunderMath<MockMegaThunderRand>;
type TestContext = DispatcherSyncContext<Math, ServerConfig<DispatcherContext>, ServerConfig<DispatcherContext>, SuccessStateLoader>;
type TestConfig = ServerConfig<TestContext>;

async fn create_server_cfg() -> TestConfig {
    let pool = create_connection().await;
    let cfg = ServerConfig::new(ServerArg {
        pool: pool.clone(),
        ..Default::default()
    })
    .await
    .expect("error create config");

    let game = cfg.game_service.get_game(GAME_NAME).await.expect("error load game!").expect("error find game!");

    let mut rand = MockMegaThunderRand::new();
    rand.expect_rand_cols_group().return_const(Ok((vec![0; gconf::CFG.reels[0].len()], (0..gconf::CFG.reels[0].len()).map(|_| vec!['H'; gconf::ROWS]).collect())));

    rand.expect_rand_mults().return_const(Ok((0..gconf::CFG.reels[0].len()).map(|_| vec![0; gconf::ROWS]).collect()));
    rand.expect_rand_lifts().return_const(Ok((0..gconf::CFG.reels[0].len()).map(|_| vec![0; gconf::ROWS]).collect()));

    let math = MegaThunderMath::configured(rand).expect("math load error!");
    let dispatcher = cfg.create_slot_dispatcher(math, game).await.expect("error dispatcher load!");
    let dispatch_sync_ctx = DispatcherSyncContext::new(dispatcher).await;
    ServerConfig::custom(
        ServerArg {
            pool,
            ..Default::default()
        },
        Arc::new(dispatch_sync_ctx),
    )
    .await
    .expect("error ctx create!")
}

fn parse_list(p: &str) -> VecDeque<Value> {
    let file = File::open(format!("packets/{GAME_FOLDER}/{p}")).unwrap();
    let reader = BufReader::new(file);
    let list: VecDeque<Value> = serde_json::from_reader(reader).unwrap();
    list
}

#[tokio::test]
async fn test_spin_no_win() {
    test_series("00-no_win.json").await;
}

#[tokio::test]
async fn test_spin_win() {
    test_series("01-win.json").await;
}

#[tokio::test]
async fn test_spin_bonus_win() {
    test_series("02-fs.json").await;
}

async fn test_series(name: &str) {
    let cfg = create_server_cfg().await;

    let http_cfg = create_http_cfg();
    let url_on = http_cfg.url_handle();
    let (s, r) = oneshot::channel();
    let dispatcher_ctx = cfg.dispatcher_context.clone();
    let (started, r_start) = oneshot::channel();
    tokio::spawn(async move {
        started.send(()).expect("error start!");
        create_server(cfg, http_cfg, r).await
    });
    r_start.await.expect("error start!");
    let (token, request_id) = assert_series(
        &url_on,
        "packets-init.json",
        None,
        None,
        dispatcher_ctx.clone(),
        vec!["wins", "balance", "userid", "roundId", "reels", "possBets", "nickname", "grid", "mults", "stops", "currBet", "holds", "gameId"],
        0,
    )
    .await;
    assert_series(
        &url_on,
        name,
        token,
        request_id,
        dispatcher_ctx.clone(),
        vec![
            "wins", "userid", "roundId", "balance", "category",
            //remove
        ],
        1,
    )
    .await;
    s.send(()).expect("error send");
}

async fn assert_series(
    url_on: &str,
    name: &str,
    auth_token: Option<String>,
    request_id: Option<String>,
    ctx: Arc<TestContext>,
    excludes: Vec<&str>,
    offset: usize,
) -> (Option<String>, Option<String>) {
    println!("assert: {name}");
    let packets = parse_list(name);
    if let Some(_t) = auth_token.as_ref() {
        let out = &packets[0]["out"].as_array().unwrap();
        let first = &out[0];
        let balance = first["balance"].as_i64().unwrap();
        let balance_on = Decimal::new(balance, 2);

        ctx.set_balance(balance_on);
    }

    let client = Client::new();
    let mut token: Option<String> = auth_token;
    let mut id: Option<String> = request_id;
    for i in offset..packets.len() {
        println!("packet: {i}");
        let p = &packets[i];
        let input = &p["in"];
        let expected = &p["out"];
        if expected[0]["subType"] == "SPIN" || expected[0]["subType"] == "RESPIN" {
            //println!("prewiev: {}", expected[0]);
            let game_data = parse_game_data::<Math>(expected[0].clone());
            let spin_data = match game_data {
                GameData::Spin(d) => d,
                GameData::ReSpin(d) => d,
                _ => panic!("illegal game data!"),
            };
            let stops = spin_data.result.stops.clone();
            let grid = spin_data.result.grid.clone();

            let mut rand = MockMegaThunderRand::new();
            rand.expect_rand_cols_group().return_const(Ok((stops.clone(), grid.clone())));
            rand.expect_rand_cols().return_const((stops.clone(), grid.clone()));

            if let Some(s) = spin_data.result.special {
                let mults = s.mults.clone();
                let lifts = s.lifts.clone();
                rand.expect_rand_mults().return_const(Ok(mults.clone()));
                rand.expect_rand_lifts().return_const(Ok(lifts.clone()));

                rand.expect_rand_over().return_const(Ok(s.overlay));
            }
            ctx.set_random(rand);
        }

        let request_on = Url::parse(url_on).unwrap();
        let mut headers = HeaderMap::new();
        if let Some(t) = token.clone() {
            headers.insert(AUTH_TOKEN, HeaderValue::from_str(t.as_str()).expect("error header"));
        }
        if let Some(t) = id.clone() {
            headers.insert(REQUEST_ID, HeaderValue::from_str(t.as_str()).expect("error header"));
        }
        let response = client.post(request_on).headers(headers).json(&input).send().await.expect("error send");
        if let Some(t) = response.headers().get(AUTH_TOKEN) {
            token = Some(t.to_str().expect("error").to_string());
        }
        if let Some(t) = response.headers().get(REQUEST_ID) {
            id = Some(t.to_str().expect("error").to_string());
        }
        let text = response.text().await.unwrap();

        let actual: Value = serde_json::from_str(&text).expect("error parse json");
        println!("expected: {expected}");
        println!("actual: {actual}");
        assert_answer(&expected, &actual, "{}".to_string(), &excludes);

        if expected[0]["nextAct"] == "COLLECT" {
            println!("collect...");
            let request_on = Url::parse(url_on).unwrap();
            let mut headers = HeaderMap::new();
            if let Some(t) = token.clone() {
                headers.insert(AUTH_TOKEN, HeaderValue::from_str(t.as_str()).expect("error header"));
            }
            if let Some(t) = id.clone() {
                headers.insert(REQUEST_ID, HeaderValue::from_str(t.as_str()).expect("error header"));
            }
            let response = client.post(request_on).headers(headers).json(&create_collect::<Math>()).send().await.expect("error send");
            if let Some(t) = response.headers().get(REQUEST_ID) {
                id = Some(t.to_str().expect("error").to_string());
            }
            let _collect_text = response.text().await.unwrap();
        }
    }
    (token, id)
}
