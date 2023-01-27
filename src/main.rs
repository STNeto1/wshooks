extern crate argon2;
extern crate dotenv;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{
        Path,
        State, ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    Router,
    routing::{get, post},
};
//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use dotenv::dotenv;
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tower_http::trace::TraceLayer;
use tracing::{info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::http;
use prisma::PrismaClient;

mod auth;
mod errors;
mod prisma;

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

    let client = PrismaClient::_builder()
        .build()
        .await
        .expect("failed to create prisma");
    let (tx, rx) = broadcast::channel::<WSData>(100);

    let app_state = Arc::new(AppState { tx, rx, client });

    // build our application with some routes
    let app = Router::new()
        .route("/", get(index))
        .route("/:key", get(key_handler))
        .route("/ws", get(ws_handler))
        .route("/auth/login", post(http::login))
        .route("/auth/register", post(http::register))
        .route("/auth/profile", get(http::profile))
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

async fn key_handler(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.tx.send(WSData {
        key,
        data: String::from("data"),
    }) {
        Ok(_) => {
            println!("success sending message");
        }
        Err(err) => println!("error sending message => {}", err),
    }
    return (StatusCode::OK, String::from("key route"));
}

/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

/// Actual websocket statemachine (one will be spawned per connection)
async fn handle_socket(socket: WebSocket, who: SocketAddr, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    //send a ping (unsupported by some browsers) just to kick things off and get a response
    if let Ok(_) = sender.send(Message::Ping("Ping".into())).await {
        println!("Pinged {}...", who);
    } else {
        println!("Could not send ping {}!", who);
        // no Error here since the only thing we can do is to close the connection.
        // If we can not send messages, there is no way to salvage the statemachine anyway.
        return;
    }

    // receive single message from a client (we can either receive or send with socket).
    // this will likely be the Pong for our Ping or a hello message from client.
    // waiting for message from a client will block this task, but will not block other client's
    // connections.
    if let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            let ping_result = process_message(msg, who);
            if ping_result.should_stop() {
                return;
            }
        } else {
            println!("client {} abruptly disconnected", who);
            return;
        }
    }

    let mut _ref = String::new();
    if let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            let client_ref = process_message(msg, who);
            // if is not a string with the client ref, just kill
            match client_ref.get_data() {
                Some(d) => _ref = d,
                _ => return,
            }
        } else {
            println!("client {} abruptly disconnected", who);
            return;
        }
    }

    println!("client ref => {}", _ref);

    let mut rx = state.rx.resubscribe();
    let send_message_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // client should ignore message not sent to his ref
            if msg.key != _ref {
                continue;
            }

            if sender
                .send(Message::Text(format!(
                    "key => {}, value => {}",
                    msg.key, msg.data
                )))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // keep the task of sending messages back to client
    let _ = send_message_task.await;

    // returning from the handler closes the websocket connection
    println!("Websocket context {} destroyed", who);
}

#[derive(Clone)]
struct WSData {
    key: String,
    data: String,
}

#[derive(PartialEq)]
enum MessagePayload {
    Data(String),
    End,
    Ignore,
}

impl MessagePayload {
    fn should_stop(&self) -> bool {
        return self == &MessagePayload::End;
    }

    fn get_data(&self) -> Option<String> {
        return match self {
            MessagePayload::Data(data) => Some(data.to_owned()),
            _ => None,
        };
    }
}

/// helper to print contents of messages to stdout. Has special treatment for Close.
fn process_message(msg: Message, who: SocketAddr) -> MessagePayload {
    return match msg {
        Message::Text(t) => MessagePayload::Data(t),
        Message::Binary(_) => MessagePayload::End,
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> {} sent close with code {} and reason `{}`",
                    who, cf.code, cf.reason
                );
            } else {
                println!(">>> {} somehow sent close message without CloseFrame", who);
            }
            MessagePayload::End
        }
        Message::Pong(_) => MessagePayload::Ignore,
        Message::Ping(_) => MessagePayload::Ignore,
    };
}
