pub mod adapters;
pub mod application;
pub mod domain;

#[tokio::main]
async fn main() {
    let app = adapters::inbound::http::router();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
