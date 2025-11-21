mod utils;

use futures_util::lock::Mutex;
use futures_util::{StreamExt, SinkExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

use tokio::sync::{broadcast, mpsc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use std::fs;
use crate::utils::os_check::{Edge, edge_check};


#[derive(Serialize)]
#[serde(tag = "action")]
enum ServerMsg {
    #[serde(rename = "get_tabs")]
    GetTabs {edge : String},
}

#[derive(Deserialize, Debug)]
#[serde(tag = "action")]
enum ClientMsg {
    //THIS IS VARIANT SO { action : tabs , tabs : [...] , edge : left}
    #[serde(rename = "tabs")]
    Tabs { tabs: Vec<String>  , edge : String},

    // AND THIS WILL BE {action : edge , tabs : [...]}
    // #[serde(rename = "edge")]
    // Edge { tabs: Vec<String> },
}

#[derive(Debug)]
struct DeviceInfo {
    ip: String,
    tx : mpsc::Sender<String>,
}
type DeviceMap = Arc<Mutex<HashMap<String , DeviceInfo>>>;
#[tokio::main]
async fn main() {

    let (local_tx, _) = broadcast::channel::<String>(16);
    let (global_tx, _) = broadcast::channel::<String>(16);

    let _keep_local_alive = local_tx.clone();
    let _keep_global_alive = global_tx.clone();

    let device_map  : DeviceMap = Arc::new(Mutex::new(HashMap::new()));

    let screen_config =  load_config().unwrap_or_else(|e| {
        eprintln!("[CONFIG] Config load error : {}" , e);
        std::process::exit(1);
    });
    let screen_config_map = build_map(&screen_config);

    // [edge_checker] ----- local_channel ----> ws 
    {
        let local_tx_clone = local_tx.clone();
        edge_check(move |edge| {

            // edge_checker ----- [local_channel] ----> ws 
            match edge {
                Edge::Left => {
                    let _  = local_tx_clone.send("get_tabs-left".to_string());
                }
                Edge::Right => {
                    let _ = local_tx_clone.send("get_tabs-right".to_string());
                }
            }
        });
    }

    //websocket listner
    //[ws] ---- global_boardcast ---- TCP ----> another computer 
    {
        let local_tx_clone = local_tx.clone();
        let device_map_for_ws = device_map.clone();

        tokio::spawn( async move {
            let url = "0.0.0.0:24810";
            let listener: TcpListener = TcpListener::bind(url).await.expect("[WS] can't bind with this addr");

            println!("[WS] listening @ {}" , url);

            while let Ok((stream , peer_addr)) = listener.accept().await {
                println!("[WS] accept conn from {}" , peer_addr);
                tokio::spawn(handle_ws(stream , peer_addr , local_tx_clone.clone(), device_map_for_ws.clone()));
            }
        });
    }


    // tcp (global)
    {
        let global_tx_clone = global_tx.clone();
        let device_map_clone = device_map.clone();
        let screen_config_map_clone = screen_config_map.clone();

        tokio::spawn(async move {
            let url: &str = "0.0.0.0:24811";
            let listener: TcpListener = TcpListener::bind(url).await.expect("[tcp] can't bind with this addr");
            println!("[TCP] listening @ {}" , url);

            while let Ok((mut stream , addr)) = listener.accept().await {

                println!("[TCP] connection from : {}" , addr);
                let ip = addr.ip().to_string();

                //channel
                let mut global_recv = global_tx_clone.subscribe();
                let (tx , mut rx) = mpsc::channel::<String>(32);

                //compare with config set edge accordingly
                if let Some(edge) = screen_config_map_clone.get(&ip) {
                    println!("[CONFIG] registor from : {}" , addr);
                    device_map_clone.lock().await.insert(
                        edge.clone(),
                        DeviceInfo { ip: ip.clone() , tx},
                    );
                }
                
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        tokio::select! {
                            //ws ---- [global_boardcast] ---- TCP ----> another computer 
                            Ok(msg) = global_recv.recv() => {

                                // ws --- global boardcast ---- [TCP] -----> another computer
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    println!("[TCP] forwarding fail @ global channel : {}" , e);
                                    break;
                                }
                            }

                            //use some cause it cannot fail
                            //recv chrome_ext ---- ws ----> forwarder ---- private_channel ----- [tcp] ----> another_computer 
                            Some(msg) = rx.recv() => {

                                
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    print!("[TCP] forwarding fail @ private channel : {}" , e);
                                    break;
                                }
                            }

                            //another computer ---- [TCP] ----> local computer
                            result = stream.read(&mut buf) => {
                                match result {
                                    Ok(0) => {
                                        println!("[TCP] peer disconnect ; {}" , addr);
                                        break;
                                    }
                                    Ok(n) => {
                                        let msg_str = String::from_utf8_lossy(&buf[..n]);
                                        println!("[TCP] Received via TCP : {}" , msg_str);
                                    }

                                    Err(e) => {
                                        println!("[TCP] receive msg err : {}" , e);
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
    device_map : DeviceMap,
    ) {

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("[WS][handle_ws] handshake err : {} (from {})" , e , peer_addr);
            return;
        }
    };
    let (mut ws_sender , mut ws_reciver) = ws_stream.split();
    let mut local_recv = local_tx.subscribe();

    loop {
        tokio::select! {

            //why use - instead of json welll.... i don't know i expect this to be faster ;)
            //TODO - find evident to back this up ¯\(ツ)/¯

            //edge checker ----- [local_channel] -----> forwarder --- ws ---> chrome_ext
            Ok(msg) = local_recv.recv() => {
                if let Some(msg_type) = msg.split('-').next() {
                    if msg_type == "get_tabs" {

                        //edge checker ----- local_channel -----> [forwarder] --- ws ---> chrome_ext
                        let edge = msg.split('-').nth(1).unwrap_or("");
                        match serde_json::to_string(&ServerMsg::GetTabs {edge : edge.to_string()}) {
                            Ok(json_msg) => {
                                println!("[WS] send to get_tabs to {}" , peer_addr);

                                //edge checker ----- local_channel -----> forwarder --- [ws] ---> chrome_ext
                                if let Err(e) = ws_sender.send(Message::Text(json_msg)).await {
                                    eprintln!("[WS] fail to send msg to {} , err : {}" , peer_addr , e);
                                    break; //ws prob disconnect so we break
                                }
                            }
                            Err(e) => {
                                eprintln!("[serde_json] fail to convert to json {} , err : {}" , msg , e );
                                continue;
                            }
                        }

                    }
                }
            }

            //recv chrome_ext ---- [ws] ----> forwarder ----- private_channel ---- tcp ----> another_computer 
            Some(msg) = ws_reciver.next() => {

                println!("[WS] forwarding from perr {}" , peer_addr);
                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        println!("[WS] Error reciving msg err : {} ,  from {} " , e , peer_addr);
                        break;
                    }
                };

                //recv chrome_ext ---- ws ----> [forwarder] ----  private_channel ----- tcp ----> another_computer
                //convert edge --> tcp specific stream 
                if let Message::Text(text) = msg {
                    match serde_json::from_str::<ClientMsg>(&text) {
                        Ok(ClientMsg::Tabs {tabs , edge}) => {
                            let json = serde_json::to_string(&tabs).unwrap();
                            let map_guard = device_map.lock().await;

                            if let Some(device) = map_guard.get(&edge) {

                                //recv chrome_ext ---- ws ----> forwarder ---- [private_channel] ----- tcp ----> another_computer 
                                if let Err(e) = device.tx.send(json.clone()).await {
                                    println!("[TCP][GLOBAL_CHANNEL] Fail to send ip : {} , edge : {}, Err : {}" , device.ip , edge , e);
                                }

                            } else {
                                println!("[TCP][CONFIG] Target device with edge : {} not found in " ,edge);
                            }
                        }
                        Err(e) => {
                            println!("[serde json] Failed to parse JSON from {}: {}", peer_addr, e);
                            println!("[serde json] Raw message was: {}", text);
                        }
                    }
                } else if msg.is_close() {
                    println!("[WS] {} disconnected.", peer_addr);
                    break;
                }
            }

        }
    }
    
}

//==== handle config =====
#[derive(Debug, Deserialize)]
struct Config {
    devices: Vec<Device>
}
#[derive(Debug, Deserialize)]
struct Device {
    ip : String,
    edge: String,
}

fn load_config() -> anyhow::Result<Config> {
    let content = fs::read_to_string("config.toml")?;
    let config : Config = toml::from_str(&content)?; //if Err -return
    Ok(config) 
}
fn build_map(config : &Config) -> HashMap<String , String> {
    let mut map = HashMap::new();

    for device in &config.devices {
        map.insert(device.ip.clone(), device.edge.clone());
    }

    map
}