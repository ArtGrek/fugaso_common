use std::sync::Arc;
use async_trait::async_trait;
use essential_rand::random::RandomGenerator;
use essential_data::sequence_generator::{RequestId, SequenceGenerator};
use sea_orm::DbErr;
use log::error;
use tokio::sync::{mpsc, oneshot};

pub struct FugasoIdGenerator {
    pub common_round_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub round_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub action_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub gain_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub promo_account_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub promo_stats_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
    pub promo_tran_id_gen: Box<dyn SequenceGenerator + Send + Sync>,
}

#[async_trait]
pub trait IdGenerator {
    async fn gen_common_round(&self) -> Result<i64, DbErr>;

    async fn gen_action(&self) -> Result<i64, DbErr>;

    async fn gen_round(&self) -> Result<i64, DbErr>;

    async fn gen_gain(&self) -> Result<i64, DbErr>;

    async fn gen_promo_account(&self) -> Result<i64, DbErr>;

    async fn gen_promo_stats(&self) -> Result<i64, DbErr>;

    async fn gen_promo_transaction(&self) -> Result<i64, DbErr>;
}

#[async_trait]
impl IdGenerator for FugasoIdGenerator {
    async fn gen_common_round(&self) -> Result<i64, DbErr> {
        self.common_round_id_gen.generate().await
    }

    async fn gen_action(&self) -> Result<i64, DbErr> {
        self.action_id_gen.generate().await
    }

    async fn gen_round(&self) -> Result<i64, DbErr> {
        self.round_id_gen.generate().await
    }

    async fn gen_gain(&self) -> Result<i64, DbErr> {
        self.gain_id_gen.generate().await
    }

    async fn gen_promo_account(&self) -> Result<i64, DbErr> {
        self.promo_account_id_gen.generate().await
    }

    async fn gen_promo_stats(&self) -> Result<i64, DbErr> {
        self.promo_stats_id_gen.generate().await
    }

    async fn gen_promo_transaction(&self) -> Result<i64, DbErr> {
        self.promo_tran_id_gen.generate().await
    }
}

pub struct DemoIdGenerator {
    sender: mpsc::UnboundedSender<RequestId>,
}

impl DemoIdGenerator {
    pub fn new() -> Self {
        let (s, mut r) = mpsc::unbounded_channel::<RequestId>();
        tokio::spawn(async move {
            let mut rand = RandomGenerator::new();
            while let Some(r) = r.recv().await {
                if let Err(_) = r.sender.send(Ok(rand.random_i64())) {
                    error!("error send from demo generator")
                }
            }
        });
        Self {
            sender: s,
        }
    }

    async fn generate(&self) -> Result<i64, DbErr> {
        let (sd, rc) = oneshot::channel();
        self.sender.send(RequestId { sender: sd }).map_err(|e| DbErr::Custom(format!("error generate id {}", e)))?;
        rc.await.map_err(|e| DbErr::Custom(format!("error generate id {}", e)))?
    }
}

#[async_trait]
impl IdGenerator for DemoIdGenerator {
    async fn gen_common_round(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_action(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_round(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_gain(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_promo_account(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_promo_stats(&self) -> Result<i64, DbErr> {
        self.generate().await
    }

    async fn gen_promo_transaction(&self) -> Result<i64, DbErr> {
        self.generate().await
    }
}

pub trait IdGeneratorFactory {
    fn create(&self, demo: bool) -> Arc<dyn IdGenerator + Send + Sync>;
}