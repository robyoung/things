use std::net::SocketAddr;

use axum::{response::IntoResponse, routing::get, Json, Router};
use things_api::{Item, List};

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
    Json(List {
        name: "example".to_owned(),
        items: vec![
            Item {
                value: "first".to_owned(),
            },
            Item {
                value: "second".to_owned(),
            },
            Item {
                value: "third".to_owned(),
            },
        ],
    })
}
