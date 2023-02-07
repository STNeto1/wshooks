extern crate argon2;
extern crate dotenv;

use std::{collections::HashMap, net::SocketAddr, sync::Arc, vec};

use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, method_routing::delete, post},
    Router,
};
use dotenv::dotenv;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use prisma::PrismaClient;

use crate::ws::{get_key_handler, post_key_handler, ws_handler, WSData};

mod auth;
mod errors;
mod prisma;
mod topic;
mod ws;

pub struct AppState {
    tx: Sender<WSData>,
    rx: Receiver<WSData>,
    client: PrismaClient,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_tracing_aka_logging=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenv().expect("failed to load env");

    let thread_client = PrismaClient::_builder()
        .build()
        .await
        .expect("failed to create prisma");
    let client = PrismaClient::_builder()
        .build()
        .await
        .expect("failed to create prisma");
    let (tx, rx) = broadcast::channel::<WSData>(100);

    let mut thread_rx = tx.subscribe();
    tokio::spawn(async move {
        while let Ok(msg) = thread_rx.recv().await {
            let new_log = thread_client
                .log()
                .create(
                    msg.key,
                    msg.body,
                    serde_json::to_value(msg.query).unwrap_or_default(),
                    serde_json::to_value(msg.headers).unwrap_or_default(),
                    vec![],
                )
                .exec()
                .await;

            match new_log {
                Ok(_) => (),
                Err(e) => println!("error creating log {}", e),
            }
        }

        println!("thread handler received err");
    });

    let app_state = Arc::new(AppState { tx, rx, client });

    // build our application with some routes
    let app = Router::new()
        .route("/", get(index))
        .route("/:key", get(get_key_handler).post(post_key_handler))
        .route("/ws", get(ws_handler))
        // .route("/auth/login", post(auth::http::login))
        // .route("/auth/register", post(auth::http::register))
        // .route("/auth/profile", get(auth::http::profile))
        // .route("/topic/create", post(topic::http::create_topic))
        // .route("/topic/list", get(topic::http::list_user_topics))
        // .route("/topic/show/:id", get(topic::http::show_user_topic))
        // .route("/topic/delete/:id", delete(topic::http::delete_user_topic))
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    // run it with hyper
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("server running on {}", addr.to_string());
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

async fn index() -> impl IntoResponse {
    return (StatusCode::OK, String::from("index route"));
}
