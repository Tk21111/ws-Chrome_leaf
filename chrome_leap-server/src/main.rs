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
        eprintln!("[config] Config load error : {}" , e);
        std::process::exit(1);
    });
    let screen_config_map = build_map(&screen_config);

    // function ----- local_tx ----> ws 
    {
        let local_tx_clone = local_tx.clone();
        edge_check(move |edge| {
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
    {
        let global_tx_clone = global_tx.clone();
        let local_tx_clone = local_tx.clone();
        let device_map_for_ws = device_map.clone();

        tokio::spawn( async move {
            let url = "0.0.0.0:24810";
            let listener: TcpListener = TcpListener::bind(url).await.expect("[ws] can't bind with this addr");

            println!("[ws] listening @ {}" , url);

            while let Ok((stream , peer_addr)) = listener.accept().await {
                println!("[ws] accept conn from {}" , peer_addr);
                tokio::spawn(handle_ws(stream , peer_addr , local_tx_clone.clone() , global_tx_clone.clone() , device_map_for_ws.clone()));
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

                let ip = addr.ip().to_string();

                let (tx , mut rx) = mpsc::channel::<String>(32);

                //compare with config set edge accordingly
                if let Some(edge) = screen_config_map_clone.get(&ip) {
                    device_map_clone.lock().await.insert(
                        edge.clone(),
                        DeviceInfo { ip: ip.clone() , tx},
                    );
                }
                
                let mut global_recv = global_tx_clone.subscribe();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        tokio::select! {
                            //ws ---- global boardcast ---- TCP ----> another computer 
                            Ok(msg) = global_recv.recv() => {
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    println!("[TCP] send msg Err : {}" , e);
                                    break;
                                }
                            }

                            //use some cause it cannot fail
                            Some(msg) = rx.recv() => {
                                if let Err(e) = stream.write_all(msg.as_bytes()).await {
                                    print!("[TCP] forward from local channel Err : {}" , e);
                                    break;
                                }
                            }

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
    global_tx : broadcast::Sender<String>,
    device_map : DeviceMap,
    ) {

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("[ws] handshake err : {} (from {})" , e , peer_addr);
            return;
        }
    };

    let (mut ws_sender , mut ws_reciver) = ws_stream.split();

    let mut local_recv = local_tx.subscribe();
    loop {
        tokio::select! {

            //recv broadcast edge check --- ws ---> chrome ext
            //why use - instead of json welll.... i don't know i expect this to be faster ;)
            //TODO - find evident to back this up ¯\(ツ)/¯
            Ok(msg) = local_recv.recv() => {
                if let Some(msg_type) = msg.split('-').next() {
                    if msg_type == "get_tabs" {
                        let edge = msg.split('-').nth(1).unwrap_or("");
                        match serde_json::to_string(&ServerMsg::GetTabs {edge : edge.to_string()}) {
                            Ok(json_msg) => {
                                if let Err(e) = ws_sender.send(Message::Text(json_msg)).await {
                                    eprintln!("[ws] fail to send msg to {} , err : {}" , peer_addr , e);
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

            //recv chrome ext ---- ws ----> local computer ---- tcp ----> global computer 
            Some(msg) = ws_reciver.next() => {
                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        println!("[ws] Error reciving msg err : {} ,  from {} " , e , peer_addr);
                        break;
                    }
                };

                if let Message::Text(text) = msg {
                    match serde_json::from_str::<ClientMsg>(&text) {
                        
                        //local computer ---- tcp ----> global computer
                        Ok(ClientMsg::Tabs {tabs , edge}) => {
                            let json = serde_json::to_string(&tabs).unwrap();
                            let _ = global_tx.send(json.clone());

                            let map_guard = device_map.lock().await;

                            if let Some(device) = map_guard.get(&edge) {
                                if let Err(e) = device.tx.send(json.clone()).await {
                                    println!("[tcp] Fail to send ip : {} , edge : {}, Err : {}" , device.ip , edge , e);
                                }
                            } else {
                                println!("[device_map] Target device with edge : {} not found" ,edge);
                            }

                        }
                        Err(e) => {
                            println!("[serde json] Failed to parse JSON from {}: {}", peer_addr, e);
                            println!("[serde json] Raw message was: {}", text);
                        }
                    }
                } else if msg.is_close() {
                    println!("[tcp] {} disconnected.", peer_addr);
                    break;
                }
            }

        }
    }
    
}


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