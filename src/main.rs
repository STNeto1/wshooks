use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        TypedHeader,
    },
    response::IntoResponse,
    routing::get,
    Router,
};

use std::borrow::Cow;
use std::net::SocketAddr;
use std::ops::ControlFlow;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::CloseFrame;

//allows to split the websocket stream into separate TX and RX branches
use futures::{sink::SinkExt, stream::StreamExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_websockets=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // build our application with some routes
    let app = Router::new()
        .route("/ws", get(ws_handler))
        // logging so we can see whats going on
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        );

    // run it with hyper
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    println!("`{}` at {} connected.", user_agent, addr.to_string());
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| handle_socket(socket, addr))
}

/// Actual websocket statemachine (one will be spawned per connection)
async fn handle_socket(mut socket: WebSocket, who: SocketAddr) {
    //send a ping (unsupported by some browsers) just to kick things off and get a response
    if let Ok(_) = socket.send(Message::Ping("Ping".into())).await {
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
    if let Some(msg) = socket.recv().await {
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

    let mut _ref = "".to_owned();
    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            let client_ref = process_message(msg, who);
            match client_ref.get_data() {
                Some(d) => _ref = d,
                _ => return,
            }
        } else {
            println!("client {} abruptly disconnected", who);
            return;
        }
    }

    println!("client ref: {}", _ref);

    // Since each client gets individual statemachine, we can pause handling
    // when necessary to wait for some external event (in this case illustrated by sleeping).
    // Waiting for this client to finish getting its greetings does not prevent other clients from
    // connecting to server and receiving their greetings.
    /*
    for i in 1..5 {
        if socket
            .send(Message::Text(String::from(format!("Hi {} times!", i))))
            .await
            .is_err()
        {
            println!("client {} abruptly disconnected", who);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }*/

    // returning from the handler closes the websocket connection
    println!("Websocket context {} destroyed", who);
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
        match self {
            MessagePayload::Data(data) => return Some(data.to_owned()),
            _ => return None,
        }
    }
}

/// helper to print contents of messages to stdout. Has special treatment for Close.
fn process_message(msg: Message, who: SocketAddr) -> MessagePayload {
    match msg {
        Message::Text(t) => return MessagePayload::Data(t),
        Message::Binary(_) => return MessagePayload::End,
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> {} sent close with code {} and reason `{}`",
                    who, cf.code, cf.reason
                );
            } else {
                println!(">>> {} somehow sent close message without CloseFrame", who);
            }
            return MessagePayload::End;
        }

        Message::Pong(_) => return MessagePayload::Ignore,
        // You should never need to manually handle Message::Ping, as axum's websocket library
        // will do so for you automagically by replying with Pong and copying the v according to
        // spec. But if you need the contents of the pings you can see them here.
        Message::Ping(_) => return MessagePayload::Ignore,
    }
}
