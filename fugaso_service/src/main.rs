use fugaso_admin::config::run_server;

#[tokio::main]
async fn main() {
    run_server().await;
}
