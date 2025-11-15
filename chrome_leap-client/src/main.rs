use futures_util::{StreamExt, SinkExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

use tokio::sync::broadcast;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc; // A "channel" to send messages between tasks
use tokio::time::sleep;
#[derive(Serialize)]
#[serde(tag = "action")]
enum ServerMsg {
    #[serde(rename = "get_tabs")]
    GetTabs,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "action")]
enum ClientMsg {
    #[serde(rename = "tabs")]
    Tabs { tabs: Vec<String> },
}

#[tokio::main]
async fn main() {

    let (broadcast_tx, _) = broadcast::channel::<String>(16);

    //test only
    //TODO - remplace this 
    let tx_clone_for_simulator = broadcast_tx.clone();
    tokio::spawn(async move {
        // This task will run forever
        loop {
            // Wait for 10 seconds
            sleep(Duration::from_secs(10)).await;

            // Send the "edge hit" event to all listeners
            println!("--- GLOBAL EVENT: Simulating edge hit! Broadcasting 'get_tabs' ---");
            if let Err(e) = tx_clone_for_simulator.send("get_tabs".to_string()) {
                println!("Broadcast send error: {}", e);
            }
        }
    });

    let url = "127.0.0.1:24810";
    let listener = TcpListener::bind(url).await.expect("can't bind with this addr");

    println!("Listening @ {}" , url);
    while let Ok((stream , peer_addr)) = listener.accept().await {
        println!("accept conn from {}" , peer_addr);
        tokio::spawn(handle_conn(stream , peer_addr , broadcast_tx.clone()));
    }
} 

async fn handle_conn(stream : TcpStream , peer_addr : std::net::SocketAddr , broadcast_tx : broadcast::Sender<String>) {

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("handshake err : {} (from {})" , e , peer_addr);
            return;
        }
    };

    let (mut ws_sender , mut ws_reciver) = ws_stream.split();

    let mut broadcast_tx = broadcast_tx.subscribe();
    loop {
        tokio::select! {
            Ok(msg) = broadcast_tx.recv() => {
                if msg == "get_tabs" {

                    let msg_data = ServerMsg::GetTabs;
                    let json_msg = serde_json::to_string(&msg_data).unwrap();

                    if ws_sender.send(Message::Text(json_msg)).await.is_err() {
                        println!("fail to send msg {}" , peer_addr);
                        break;
                    }
                }
            }

            Some(msg) = ws_reciver.next() => {
                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        println!("Error reciving msg err : {} ,  from {} " , e , peer_addr);
                        break;
                    }
                };

                if let Message::Text(text) = msg {
                    match serde_json::from_str::<ClientMsg>(&text) {
                        Ok(client_msg) => {
                            match client_msg {
                                ClientMsg::Tabs {tabs} => {
                                    println!("Received tabs from {}: {:?}", peer_addr, tabs);
                                }
                            }
                        }

                        Err(e) => {
                            println!("Failed to parse JSON from {}: {}", peer_addr, e);
                            println!("Raw message was: {}", text);
                        }
                    }
                } else if msg.is_close() {
                    println!("{} disconnected.", peer_addr);
                    break;
                }
            }

        }
    }
    
}