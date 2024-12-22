use std::io::{self, Write};
use std::net::UdpSocket;
use std::thread;
use std::sync::mpsc;
use std::process::Command;

fn main() -> io::Result<()> {
    // 设置本地监听地址
    println!("输入本地监听地址: ");
    let mut local_addr_str = String::new();
    io::stdin().read_line(&mut local_addr_str)?;
    let local_addr = local_addr_str.trim();
    // 设置目标地址
    println!("输入目标地址: ");
    let mut target_addr_str = String::new();
    io::stdin().read_line(&mut target_addr_str)?;
    let target_addr = target_addr_str.trim();
    //设置昵称
    println!("输入昵称: ");
    let mut nickname = String::new();
    io::stdin().read_line(&mut nickname)?;
    let nickname = nickname.trim().to_string();
    Command::new("cmd").args(&["/C", "cls"]).status().unwrap();

    // 创建 UDP socket 并绑定到本地地址
    let socket = UdpSocket::bind(local_addr)?;
    socket.set_nonblocking(true)?;
    // 连接目标地址
    socket.connect(target_addr)?;
    // 创建通道
    let (tx, rx) = mpsc::channel();
    // 创建接收消息的线程
    let receiver_socket = socket.try_clone()?;
    let receive_handle = thread::spawn(move || {
        receive_messages(receiver_socket, tx);
    });
    // 创建刷新屏幕的线程
    let refresh_handle = thread::spawn(move || {
        refreash_screen(rx);
    });
    // 创建发送消息的线程
    let send_handle = thread::spawn(move || {
        if let Err(e) = send_messages(socket, nickname) {
            eprintln!("Failed to send message: {}", e);
        }
    });
    // 等待线程结束
    receive_handle.join().expect("Receiver thread failed");
    refresh_handle.join().expect("Refresh screen thread failed");
    send_handle.join().expect("Sender thread failed");

    Ok(())
}

fn receive_messages(socket: UdpSocket, tx: mpsc::Sender<String>) {
    let mut buf = [0; 1024];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((amt, _src)) => {
                if amt > 0 {
                    // 将接收到的消息转换为字符串
                    let msg = String::from_utf8_lossy(&buf[..amt]).to_string();
                    // 发送消息到通道
                    if tx.send(msg).is_err() {
                        break; 
                    }
                }
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            },
            Err(e) => {
                eprintln!("Failed to receive data: {}", e);
                break;
            }
        }
    }
}

fn send_messages(mut socket: UdpSocket, mut nickname: String) -> io::Result<()> {
    loop {
        print!("> ");
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = format!("{}:{}", nickname, input);
        let trimmed = input.trim();

        if trimmed.to_lowercase() == "exit" {
            break;
        }

        if !trimmed.is_empty() {
            if let Err(e) = socket.send(trimmed.as_bytes()) {
                eprintln!("Failed to send message: {}", e);
            }
        }
    }
    Ok(())
}

fn refreash_screen(rx: mpsc::Receiver<String>) {
    loop {
        match rx.recv() { 
            Ok(msg) => {
                print!("\r{}\n>", msg);
                if let Err(e) = io::stdout().flush() {
                    eprintln!("Failed to flush stdout: {}", e);
                    break;
                }
            },
            Err(_) => break, 
        }
    }
}