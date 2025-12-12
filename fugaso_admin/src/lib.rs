mod cache;
pub mod config;
pub mod database;
pub mod dispatcher;
mod logger;
pub mod manager;
pub mod route;
//mod example;

#[cfg(test)]
mod tests {
    use crate::config::{ServerArg, ServerConfig};
    use crate::manager::{TournamentAward, TournamentResult};
    use essential_data::user_user::OperatorShort;
    use fugaso_core::protocol::TournamentUserWin;
    use fugaso_math::protocol::DatabaseStore;
    use fugaso_math::protocol::FreeGame;
    use sea_orm::prelude::{Decimal, Uuid};
    use sea_orm::{ConnectOptions, Database};
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn it_works() {
        let event_1 = Uuid::new_v4();
        let event_2 = Uuid::new_v4();
        let result = TournamentResult {
            awards: vec![
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: event_1,
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: event_1,
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: Uuid::new_v4(),
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: event_1,
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: event_2,
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
                TournamentAward {
                    id: 0,
                    amount: Decimal::new(200, 0),
                    user: "toker".to_string(),
                    remote_id: Default::default(),
                    tour: "Hello".to_string(),
                    place: 1,
                    balance: Decimal::new(100, 0),
                    event_id: event_2,
                    ip: "127.0.0.1".to_string(),
                    remote_code: 0,
                },
            ],
        };
        let map: HashMap<Uuid, Vec<TournamentAward>> = result.awards.into_iter().fold(HashMap::new(), |mut acc, v| {
            if let Some(vec) = acc.get_mut(&v.event_id) {
                vec.push(v);
            } else {
                acc.insert(v.event_id, vec![v]);
            }
            acc
        });
        for (k, v) in map {
            println!("k: {k:?} v: {:?}", v.len());
        }
    }

    #[tokio::test]
    async fn test_tour() {
        let mut opt = ConnectOptions::new("postgres://osm:101918@localhost/lion".to_string());
        opt.max_connections(300)
            .min_connections(2)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(60 * 60))
            .max_lifetime(Duration::from_secs(24 * 60 * 60))
            .sqlx_logging(true);
        let pool = Database::connect(opt).await.expect("error open connection");

        let cfg = ServerConfig::new(ServerArg {
            name: "".to_string(),
            pool,
            tour_config: Default::default(),
            admin_config: Default::default(),
            ip_service_config: Default::default(),
            ..Default::default()
        })
        .await
        .unwrap();
        //let user_att_repo = cfg.base.user_attr_repo.as_ref();
        // let tours = user_att_repo.find_recursive_attrs(2356851, AttributeName::tour).await.unwrap();

        let _opers = cfg.p.user_repo.find_tuple_by_operator_id::<OperatorShort>(8894721, "EUR").await.unwrap();

        let gains = cfg.gain_repo.find_gains_by_rounds::<TournamentUserWin>(vec!["58806".to_string(), "58700".to_string()]).await.expect("gains are not found!");
        println!("gains: {}", serde_json::to_string(&gains).unwrap());
        let e = cfg.p.exchange_repo.find_by_src_dest(52, 70).await.unwrap().unwrap();
        let amount_on = Decimal::new(1000, 0) / e.rate;
        let amount = amount_on.round_dp(2);
        println!("{:?} - {:?}", amount_on, amount)
    }

    #[tokio::test]
    async fn test_free_game() {
        let db = "left=8|done=2|initial=10|symbol=?|totalWin=350|category=1";
        let free_game = FreeGame::from_db(db).expect("error parse");
        assert_eq!(8, free_game.left);
        assert_eq!(2, free_game.done);
        assert_eq!(10, free_game.initial);
        assert_eq!('?', free_game.symbol);
        assert_eq!(350, free_game.total_win);
        assert_eq!(1, free_game.category);

        assert_eq!(db, free_game.to_db().expect("error to db!"));
        println!("free {:?}", FreeGame::default());
    }

    pub trait Animal {
        type Parent: Animal;
        fn parent(&self) -> &Self::Parent;
        fn eat(&self) {
            self.parent().eat()
        }
        fn smell(&self) -> bool {
            self.parent().smell()
        }
    }

    pub struct Cat {}

    impl Animal for Cat {
        type Parent = Self;

        fn parent(&self) -> &Self::Parent {
            self
        }

        fn eat(&self) {
            if self.smell() {
                println!("cat eats...")
            } else {
                println!("cat throw...")
            }
        }

        fn smell(&self) -> bool {
            true
        }
    }

    pub struct Tiger {
        pub p: Cat,
    }

    impl Animal for Tiger {
        type Parent = Cat;
        fn parent(&self) -> &Self::Parent {
            &self.p
        }

        fn smell(&self) -> bool {
            false
        }
    }

    #[test]
    fn it_animal() {
        let cat = Cat {};
        let tiger = Tiger {
            p: Cat {},
        };

        cat.eat();
        tiger.eat();
    }

    #[test]
    fn test_skip() {
        let reel0 = vec!["A", "B", "C", "D", "E"];
        println!("{:?}", reel0.iter().skip(1).take(reel0.len() - 2).collect::<Vec<_>>());
    }
}
