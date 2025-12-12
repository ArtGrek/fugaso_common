use essential_test::database_configuration::{insert_euro_currency, setup_schema};
use fugaso_admin::database;
use fugaso_admin::{config::HttpConfig, route::client::END_ROOT};
use fugaso_core::protocol::PlayerRequest;
use fugaso_math::{math::SlotMath, protocol::GameData};
use fugaso_test::database_configuration::setup_schema_fugaso_game;
use sea_orm::{ConnectOptions, Database, DbConn};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::BufReader,
    sync::atomic::{AtomicI32, Ordering},
    time::Duration,
};

#[allow(unused)]
static PORT: AtomicI32 = AtomicI32::new(8500);

#[allow(unused)]
pub fn create_http_cfg() -> HttpConfig {
    HttpConfig {
        host: "127.0.0.1".to_string(),
        port: PORT.fetch_add(1, Ordering::SeqCst),
        path: END_ROOT.to_string(),
        cache: false,
    }
}

#[allow(unused)]
pub fn create_collect<M: SlotMath>() -> PlayerRequest<<M as SlotMath>::Input> {
    PlayerRequest::Collect::<M::Input>
}

/*#[allow(unused)]
pub async fn create_connection() -> DbConn {
    let mut opt = ConnectOptions::new("postgres://osm:101918@localhost/lion".to_owned());
    opt.max_connections(2).min_connections(2).connect_timeout(Duration::from_secs(8)).idle_timeout(Duration::from_secs(8)).max_lifetime(Duration::from_secs(8)).sqlx_logging(true);

    let db: DbConn = Database::connect(opt).await.expect("error open connection");
    db
}*/

pub async fn create_connection() -> DbConn {
    let mut opt = ConnectOptions::new("sqlite::memory:".to_owned());
    opt.max_connections(2).min_connections(2).connect_timeout(Duration::from_secs(8)).idle_timeout(Duration::from_secs(8)).max_lifetime(Duration::from_secs(8)).sqlx_logging(true);
    let pool = Database::connect(opt).await.expect("error db connect!");
    setup_schema(&pool).await;
    setup_schema_fugaso_game(&pool).await;
    insert_euro_currency(&pool).await;
    database::insert_games(&pool).await.expect("error insert!");
    pool
}

pub fn assert_answer(expected: &Value, actual: &Value, path: String, excludes: &Vec<&str>) {
    match expected {
        Value::String(s) => {
            if let Value::String(v) = actual {
                if path.ends_with("@bonus_pos") {
                    let act_set = v.split(",").map(|v| v.parse::<i32>().unwrap()).collect::<HashSet<_>>();
                    let exp_set = s.split(",").map(|v| v.parse::<i32>().unwrap()).collect::<HashSet<_>>();
                    assert_eq!(act_set, exp_set, "error set value {path}")
                } else {
                    assert_eq!(s, v, "error string value {path}")
                }
            } else {
                panic!("error expected on:{path:?} type:{expected:?} actual type:{actual:?}!")
            }
        }
        Value::Number(s) => {
            if let Value::Number(v) = actual {
                assert_eq!(s, v, "error number value {path}")
            } else {
                panic!("error expected on:{path:?} type:{expected:?} actual type:{actual:?}!")
            }
        }
        Value::Null => {
            assert_eq!(expected, actual, "error null value {path}")
        }
        Value::Bool(s) => {
            if let Value::Bool(v) = actual {
                assert_eq!(s, v, "error boolean value {path}")
            } else {
                panic!("error expected on:{path:?} type:{expected:?} actual type:{actual:?}!")
            }
        }
        Value::Object(s) => {
            if let Value::Object(v) = actual {
                for (k, p) in s {
                    if excludes.contains(&&**k) {
                        continue;
                    }
                    let a = &v.get(k).expect(&format!("error get value for {:?}", path.to_string() + "->" + k));
                    assert_answer(p, a, format!("{path}->{k}"), excludes);
                }
            } else {
                panic!("error expected on:{path:?} type:{expected:?} actual type:{actual:?}!")
            }
        }
        Value::Array(s) => {
            if let Value::Array(v) = actual {
                if s.len() != v.len() {
                    //panic!("error vec lengths on:{path} expected: {:?} actual: {:?}", s.len(), v);
                }
                for (i, p) in s.iter().enumerate() {
                    let a = &v.get(i).expect(&format!("error get value at {path}->[{i}]"));
                    assert_answer(p, a, format!("{path}->[{i}]"), excludes);
                }
            } else {
                panic!("error expected on:{path:?} type:{expected:?} actual type:{actual:?}!")
            }
        }
    }
}

#[allow(unused)]
pub fn parse_game_data<M: SlotMath>(v: Value) -> GameData<M::Special, M::Restore>
where
    M::Special: DeserializeOwned,
    M::Restore: DeserializeOwned,
{
    let game_data: Result<GameData<_, _>, _> = serde_json::from_value(v);
    game_data.expect("error read game_data!")
}

#[allow(unused)]
pub fn parse_list(folder: &str, p: &str) -> VecDeque<Value> {
    let file = File::open(format!("packets/{}/{}", folder, p)).unwrap();
    let reader = BufReader::new(file);
    let list: VecDeque<Value> = serde_json::from_reader(reader).unwrap();
    list
}

#[allow(unused)]
pub fn print_path_vals(game_name: &str, name: &str, paths: Vec<&str>) {
    for p in paths {
        let name_vec = p.split(".").collect::<Vec<_>>();
        let list = parse_list(game_name, name);
        println!("path: {p:?}");

        for (i, t) in list.into_iter().enumerate() {
            let outs = t["out"].as_array().expect("error array!");

            for v in outs {
                let mut val = v;
                let mut count = 0;
                for p in &name_vec {
                    if !val[p].is_null() {
                        val = &val[p];
                        count += 1;
                    }
                }
                if count == name_vec.len() {
                    println!("{i}-{p} = {}", val);
                }
            }
        }
    }
}
