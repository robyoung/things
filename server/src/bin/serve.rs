use std::net::SocketAddr;

use axum::{response::IntoResponse, routing::get, Json, Router};
use things_server::lists::RootList;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new().route("/list", get(get_list));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    tracing::debug!("listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_list() -> impl IntoResponse {
    let mut list = RootList::new("example").snapshot();
    list.add("first");
    list.add("second");
    list.add("third");

    Json(list)
}
