use sea_orm::{ConnectionTrait, DbBackend, DbConn, Schema};
use fugaso_data::{common_round, fugaso_game};

pub async fn setup_schema_fugaso_game(db: &DbConn) {
    let schema = Schema::new(DbBackend::Sqlite);
    let create_fugaso_game = schema.create_table_from_entity(fugaso_game::Entity);

    db.execute(db.get_database_backend().build(&create_fugaso_game))
        .await.expect("error create currency");
}

pub async fn setup_schema_common_round(db: &DbConn) {
    let schema = Schema::new(DbBackend::Sqlite);
    let create_common_round = schema.create_table_from_entity(common_round::Entity);

    db.execute(db.get_database_backend().build(&create_common_round))
        .await.expect("error create common_round");
}