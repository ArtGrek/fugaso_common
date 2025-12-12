pub mod model;
pub mod fugaso_game;
pub mod fugaso_percent;
pub mod fugaso_action;
pub mod fugaso_round;
pub mod promo_account;
pub mod promo_transaction;
pub mod promo_stats;
pub mod repo;
pub mod common_round;
pub mod sequence_generator;
pub mod tournament_gain;
pub mod launch_info;

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    //use std::fs::File;
    //use std::io::{BufReader, Read, Seek, Write};
    use std::marker::PhantomData;
    //use std::mem::size_of;
    //use essential_data::active_store::StoreValue;
    //use essential_data::repo::{StoreQuery, StoreSql};

    //use polodb_core::Database;
    use sea_orm::ActiveModelTrait;
    use sea_orm::IntoActiveModel;
    //use sea_orm::prelude::Decimal;
    use serde::{Deserialize, Serialize};

   // use crate::fugaso_round;
   // use crate::fugaso_round::{StoreModel, RoundDetail, RoundStatus};

    /*#[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "table")]
    pub enum QueryDb {
        R(StoreModel)
    }*/

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "table")]
    pub enum QueryT<AS> {
        R(AS)
    }

   /* #[derive(Debug, Serialize, Deserialize)]
    pub struct Some {
        pub detail: RoundDetail,
    }*/

    /*#[test]
    pub fn test_active_convert() {
        let some = Some {
            detail: RoundDetail::RICH,
        };
        println!("{}", serde_json::to_string(&some).unwrap());

        let active = StoreModel {
            //id: QueryValue::Unchanged(1),
            detail: StoreValue::U(RoundDetail::RICH),
            ..Default::default()
        };

        let active_rnd = fugaso_round::ActiveModel {
            id: Unchanged(1),
            detail: Unchanged(RoundDetail::RICH),
            ..Default::default()
        };
        serde_json::to_string(&active).unwrap();

        let query = QueryDb::R(StoreModel {
            //id: QueryValue::Unchanged(1),
            // detail: QueryValue::Unchanged(RoundDetail::RICH),
            ..Default::default()
        });
        let bt = serde_cbor::to_vec(&query).unwrap();
        let mut file = File::create("query.bin").unwrap();
        let length = bt.len().to_be_bytes();
        // let mut writer = BufWriter::new(file);
        file.write_all(&length).unwrap();
        file.write_all(&bt).unwrap();

        println!("{:?}", bt);
        file.rewind().unwrap();

        let query = QueryDb::R(StoreModel {
            //id: QueryValue::Unchanged(2),
            detail: StoreValue::U(RoundDetail::RICH),
            ..Default::default()
        });
        let bt = serde_cbor::to_vec(&query).unwrap();
        let length = bt.len().to_be_bytes();
        // let mut writer = BufWriter::new(file);
        file.write_all(&length).unwrap();
        file.write_all(&bt).unwrap();

        println!("{:?}", bt);
        file.rewind().unwrap();

        let file_open = File::open("query.bin").unwrap();
        let mut reader = BufReader::new(file_open);
        let mut length: [u8; size_of::<usize>()] = [0; size_of::<usize>()];
        reader.read_exact(&mut length).unwrap();
        let mut all_bytes: Vec<u8> = vec![0u8; usize::from_be_bytes(length)];
        reader.read_exact(&mut all_bytes).unwrap();
        let restored: QueryDb = serde_cbor::from_slice(&all_bytes).unwrap();
        println!("{restored:?}");
        //println!("{}", bt.len());
        assert_eq!(active_rnd, active.into_active_model())
    }*/

    /*#[derive(Debug, Serialize, Deserialize)]
    struct Book {
        title: String,
        author: String,
    }


    #[tokio::test]
    async fn test_surreal_db() {
        let active = StoreModel {
            id: StoreValue::U(1),
            detail: StoreValue::U(RoundDetail::RICH),
            ..Default::default()
        };
        fugaso_round::Entity::insert(active.into_active_model());

        let db = Database::open_file("rock.db").unwrap();
        let collection = db.collection::<QueryDb>("books");

         let docs = vec![
             QueryDb::R(
                 ActiveRound {
                      id: QueryValue::Unchanged(1),
                     detail: QueryValue::Unchanged(RoundDetail::RICH),
                     ..Default::default()
                 },
             ),
             QueryDb::R(
                 ActiveRound {
                     id: QueryValue::Unchanged(2),
                     detail: QueryValue::Unchanged(RoundDetail::RICH),
                     ..Default::default()
                 },
             )

         ];
         collection.insert_many(docs).unwrap();

        let books = collection.find(None).unwrap();
        for book in books {
            println!("name: {:?}", book);
        }
        db.collection::<QueryDb>("books").drop().unwrap();

        accept(QueryT::R(StoreModel {
            id: StoreValue::U(1),
            detail: StoreValue::U(RoundDetail::RICH),
            ..Default::default()
        }));
    }*/

    /*#[tokio::test]
    async fn test_bin() {
        let now = chrono::Local::now();
        let query = StoreQuery::<fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel>::A(StoreSql::In(
            fugaso_round::StoreModel {
                id: StoreValue::S(11),
                bet: StoreValue::S(22),
                line: StoreValue::S(2),
                timestamp_close: StoreValue::S(Some(now.naive_local())),
                timestamp_open: StoreValue::S(Some(now.naive_local())),
                game_id: StoreValue::S(Some(1)),
                user_id: StoreValue::S(Some(4)),
                denom: StoreValue::S(5),
                balance: StoreValue::S(Some(Decimal::new(45, 2))),
                reels: StoreValue::S(Some(3)),
                status: StoreValue::S(Some(RoundStatus::SUCCESS)),
                multi: StoreValue::S(2),
                detail: StoreValue::S(RoundDetail::RICH),
                common_id: StoreValue::S(Some(1)),
                bet_counter: StoreValue::S(1),
                stake: StoreValue::S(Some(6)),
                win: StoreValue::S(Some(4)),
            }
        ));
        let json_rmp = rmp_serde::to_vec_named(&query).unwrap();
        let json_cbor = serde_cbor::to_vec(&query).unwrap();
        let json_orig = serde_json::to_vec(&query).unwrap();
        let query: StoreQuery::<fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel> = rmp_serde::from_slice(&json_rmp).unwrap();
        let query_cbor: StoreQuery::<fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel> = serde_cbor::from_slice(&json_cbor).unwrap();
        let query_json: StoreQuery::<fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel,
            fugaso_round::StoreModel> = serde_json::from_slice(&json_orig).unwrap();
        println!("rmp:{} cbor:{} orig:{}", json_rmp.len(), json_cbor.len(), json_orig.len());
        println!("ratio-rmp: {}, ratio:cbor:{}", json_cbor.len() as f64 / json_orig.len() as f64,
                 json_rmp.len() as f64 / json_orig.len() as f64);
        println!("{query:?}");
        println!("{query_cbor:?}");
        println!("{query_json:?}");
    }*/

    #[allow(unused)]
    pub struct Cache<S: Debug + From<A>, A: ActiveModelTrait> {
        pub some: Vec<A>,
        pub phantom: PhantomData<S>,
    }

    #[allow(unused)]
    impl<S: Debug + From<A>, A: ActiveModelTrait> Cache<S, A> {
        pub fn store(&self, a: A) {
            let q: S = a.into();
            println!("{q:?}");
        }
    }

    #[allow(unused)]
    fn accept<A: ActiveModelTrait, T: IntoActiveModel<A> + Debug>(q: QueryT<T>) {
        println!("{q:?}")
    }
}
