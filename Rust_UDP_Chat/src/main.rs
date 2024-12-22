use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;
use warp::Filter;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ChatMessage {
    Init {
        user_name: String,
        timestamp: String,
    },
    Chat {
        user_name: String,
        content: String,
        timestamp: String,
    },
    System {
        content: String,
        timestamp: String,
    },
    Error {
        content: String,
    }
}

static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

#[tokio::main]
async fn main() {
    let (tx, _rx) = broadcast::channel(100);
    let tx = Arc::new(tx);
    let usernames = Arc::new(Mutex::new(HashSet::new()));

    let static_files = warp::path("static").and(warp::fs::dir("static"));
    let index = warp::path::end().and(warp::fs::file("static/index.html"));

    let usernames = warp::any().map(move || usernames.clone());
    let tx_filter = warp::any().map(move || tx.clone());
    
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(tx_filter)
        .and(usernames)
        .map(|ws: warp::ws::Ws, tx: Arc<broadcast::Sender<ChatMessage>>, usernames: Arc<Mutex<HashSet<String>>>| {
            ws.on_upgrade(move |socket| handle_client(socket, tx, usernames))
        });

    let routes = static_files.or(index).or(ws_route);

    println!("Server started at http://127.0.0.1:8080");
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}

async fn handle_client(
    websocket: warp::ws::WebSocket,
    tx: Arc<broadcast::Sender<ChatMessage>>,
    usernames: Arc<Mutex<HashSet<String>>>,
) {
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);
    let (mut client_ws_sender, mut client_ws_rcv) = websocket.split();
    let mut rx = tx.subscribe();
    let mut user_name = String::new();

    // 处理连接
    while let Some(result) = client_ws_rcv.next().await {
        if let Ok(msg) = result {
            if let Ok(text) = msg.to_str() {
                if let Ok(message) = serde_json::from_str::<ChatMessage>(text) {
                    match message {
                        ChatMessage::Init { user_name: name, timestamp } => {
                            // 检查用户名是否可用
                            let is_username_taken = {
                                let usernames = usernames.lock().unwrap();
                                usernames.contains(&name)
                            }; // MutexGuard在这里被释放

                            if is_username_taken {
                                let error = ChatMessage::Error {
                                    content: "该用户名已被使用，请选择其他用户名".to_string(),
                                };
                                let _ = client_ws_sender
                                    .send(warp::ws::Message::text(serde_json::to_string(&error).unwrap()))
                                    .await;
                                continue;
                            }

                            // 添加新用户名
                            {
                                let mut usernames = usernames.lock().unwrap();
                                user_name = name.clone();
                                usernames.insert(name);
                            } // MutexGuard在这里被释放

                            // 广播用户加入消息
                            let join_msg = ChatMessage::System {
                                content: format!("{} 加入了聊天室", user_name),
                                timestamp,
                            };
                            let _ = tx.send(join_msg);
                            break;
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    // 处理消息
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let msg_str = serde_json::to_string(&msg).unwrap_or_default();
            if let Err(_) = client_ws_sender
                .send(warp::ws::Message::text(msg_str))
                .await
            {
                break;
            }
        }
    });

    let tx_clone = tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_ws_rcv.next().await {
            if let Ok(text) = msg.to_str() {
                if let Ok(message) = serde_json::from_str::<ChatMessage>(text) {
                    let _ = tx_clone.send(message);
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    // 清理用户名和发送离开消息
    if !user_name.is_empty() {
        {
            let mut usernames = usernames.lock().unwrap();
            usernames.remove(&user_name);
        } // MutexGuard在这里被释放

        let leave_msg = ChatMessage::System {
            content: format!("{} 离开了聊天室", user_name),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        };
        let _ = tx.send(leave_msg);
    }

    println!("Client disconnected: {}", my_id);
}