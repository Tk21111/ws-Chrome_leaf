mod utils;

use futures_util::{StreamExt, SinkExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

use tokio::sync::broadcast;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

use crate::utils::os_check::edge_check;
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

    let (local_tx, _) = broadcast::channel::<String>(16);
    let (global_tx, _) = broadcast::channel::<String>(16);

    let _keep_local_alive = local_tx.clone();
    let _keep_global_alive = global_tx.clone();

    
    // function ----- local_tx ----> ws 
    {
        let local_tx_clone = local_tx.clone();
        edge_check(move || {
            let _  = local_tx_clone.send("get_tabs".to_string());
        });
    }

    //websocket listner
    {
        let global_tx_clone = global_tx.clone();
        let local_tx_clone = local_tx.clone();

        tokio::spawn( async move {
            let url = "0.0.0.0:24810";
            let listener: TcpListener = TcpListener::bind(url).await.expect("can't bind with this addr");

            println!("WS listening @ {}" , url);

            while let Ok((stream , peer_addr)) = listener.accept().await {
                println!("accept conn from {}" , peer_addr);
                tokio::spawn(handle_ws(stream , peer_addr , local_tx_clone.clone() , global_tx_clone.clone()));
            }
        });
    }

    // tcp (global)
    {
        let global_tx_clone = global_tx.clone();
        tokio::spawn(async move {
            let url: &str = "0.0.0.0:24811";
            let listener: TcpListener = TcpListener::bind(url).await.expect("can't bind with this addr");
            println!("TCP listening @ {}" , url);

            while let Ok((mut stream , addr)) = listener.accept().await {
                let mut global_recv = global_tx_clone.subscribe();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        tokio::select! {
                            //ws ---- global boardcast ---- TCP ----> another computer 
                            Ok(msg) = global_recv.recv() => {
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    println!("TCP send Err : {}" , e);
                                    break;
                                }
                            }

                            result = stream.read(&mut buf) => {
                                match result {
                                    Ok(0) => {
                                        println!("TCP peer disconnect ; {}" , addr);
                                        break;
                                    }
                                    Ok(n) => {
                                        let msg_str = String::from_utf8_lossy(&buf[..n]);
                                        println!("Received via TCP : {}" , msg_str);
                                    }

                                    Err(e) => {
                                        println!("TCP err : {}" , e);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
    }    

    loop {
        sleep(Duration::from_secs(60)).await;
    }
    
} 

async fn handle_ws(
    stream : TcpStream, 
    peer_addr : std::net::SocketAddr, 
    local_tx : broadcast::Sender<String>, 
    global_tx : broadcast::Sender<String>) {

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("handshake err : {} (from {})" , e , peer_addr);
            return;
        }
    };

    let (mut ws_sender , mut ws_reciver) = ws_stream.split();

    let mut local_recv = local_tx.subscribe();
    loop {
        tokio::select! {

            //recv broadcast edge check --- ws ---> chrome ext
            Ok(msg) = local_recv.recv() => {
                if msg == "get_tabs" {
                    let json_msg = serde_json::to_string(&ServerMsg::GetTabs).unwrap();
                    if ws_sender.send(Message::Text(json_msg)).await.is_err() {
                        println!("fail to send msg {}" , peer_addr);
                        break;
                    }
                }
            }

            //recv chrome ext ---- ws ----> local computer ---- tcp ----> global computer 
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
                        
                        //local computer ---- tcp ----> global computer
                        Ok(ClientMsg::Tabs {tabs}) => {
                            let json = serde_json::to_string(&tabs).unwrap();
                            let _ = global_tx.send(json);
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