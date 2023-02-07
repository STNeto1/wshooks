use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    body::{Body, Bytes},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{Request, StatusCode},
    response::IntoResponse,
    Json,
};
//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use axum::extract::Path;
use futures::{SinkExt, StreamExt};
use headers::HeaderMap;
use serde::Serialize;
use serde_json::Value;

use crate::AppState;

pub async fn get_key_handler(
    Path(key): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.tx.send(WSData {
        key: key.to_owned(),
        body: Value::Null,
        headers: parse_header_map(&headers),
        query: params,
    }) {
        Ok(_) => {
            println!("success sending message");
        }
        Err(err) => println!("error sending message => {}", err),
    }
    return (StatusCode::OK, String::from("key route"));
}

pub async fn post_key_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state.tx.send(WSData {
        key: key.to_owned(),
        body: payload,
        headers: parse_header_map(&headers),
        query: params,
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
pub async fn ws_handler(
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
            // sanitize string with quotes -> "topic" should be handled as topic
            match client_ref.get_data() {
                Some(d) => _ref = sanitize_ref(&d),
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
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
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

#[derive(Clone, Serialize)]
pub struct WSData {
    pub key: String,
    pub body: Value,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
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

fn sanitize_ref(msg: &String) -> String {
    let mut msg_cpy = msg.to_owned();
    if msg.starts_with("\"") {
        msg_cpy.remove(0);
    }
    if msg.starts_with("\"") {
        msg_cpy.pop();
    }

    return msg_cpy;
}

fn parse_header_map(map: &HeaderMap) -> HashMap<String, String> {
    let mut result: HashMap<String, String> = HashMap::with_capacity(map.len());
    map.keys().into_iter().for_each(|k| {
        if let Some(value) = map.get(k) {
            if let Ok(str) = value.to_str() {
                result.insert(k.to_string(), String::from(str));
            }
        }
    });

    return result;
}
