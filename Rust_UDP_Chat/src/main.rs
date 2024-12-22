use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::broadcast;
use warp::Filter;

// 为 Message 结构体添加序列化和反序列化的派生宏
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Message {
    user_name: String,
    content: String,
    timestamp: String,
}

static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

#[tokio::main]
async fn main() {
    // 创建广播通道
    let (tx, _rx) = broadcast::channel(100);
    let tx = Arc::new(tx);

    // 静态文件路由
    let static_files = warp::path("static").and(warp::fs::dir("static"));
    let index = warp::path::end().and(warp::fs::file("static/index.html"));

    // WebSocket 路由
    let tx_filter = warp::any().map(move || tx.clone());
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(tx_filter)
        .map(|ws: warp::ws::Ws, tx: Arc<broadcast::Sender<Message>>| {
            ws.on_upgrade(move |socket| handle_client(socket, tx))
        });

    // 合并所有路由
    let routes = static_files.or(index).or(ws_route);

    println!("Server started at http://127.0.0.1:8080");
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}

async fn handle_client(websocket: warp::ws::WebSocket, tx: Arc<broadcast::Sender<Message>>) {
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);
    println!("New client connected: {}", my_id);

    let (mut client_ws_sender, mut client_ws_rcv) = websocket.split();
    let mut rx = tx.subscribe();

    // 发送消息的任务
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let msg_str = serde_json::to_string(&msg).unwrap_or_default();
            if let Err(_disconnected) = client_ws_sender
                .send(warp::ws::Message::text(msg_str))
                .await
            {
                break;
            }
        }
    });

    // 接收消息的任务
    let tx_clone = tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = client_ws_rcv.next().await {
            let msg = match result {
                Ok(msg) => msg,
                Err(_) => break,
            };

            if let Ok(text) = msg.to_str() {
                let msg_result: Result<Message, _> = serde_json::from_str(text);
                if let Ok(chat_msg) = msg_result {
                    let _ = tx_clone.send(chat_msg);
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    println!("Client disconnected: {}", my_id);
}
