mod utils;

use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::{TcpStream};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use utils::chrome::open_chrome;
use dotenv::dotenv;

#[derive(Deserialize, Debug , Serialize)]
#[serde(tag = "action")]
enum GlobalMsg {
    #[serde(rename = "tabs")]
    Tabs { tabs: Vec<String> , time : String}
}

fn time_now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128 // timestamp ms
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    dotenv().ok();

    let addr = format!("{}:24811" , env::var("PORT").unwrap());

    let mut delay= 1;
    let mut stream = loop {
        match TcpStream::connect(&addr).await {
            Ok(s) => {
                println!("connected");
                break s;
            }
            Err(e) => {
                println!("Fail to connect with err : {}" , e);
                tokio::time::sleep(Duration::from_secs(delay)).await;

                delay = (delay * 2).min(30); //cap at 30 sec 
            }
        }
    };

    let reader = BufReader::new(stream);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<GlobalMsg>(&line) {
            Ok(GlobalMsg::Tabs { tabs, time }) => {
                println!("Sent time: {}", time);
                let now = time_now_ms();
                let sent_time: u128 = time.parse().unwrap();
                println!("Elapsed: {} ns", now - sent_time);
                println!("Elapsed: {:.3} ms", (now - sent_time) as f64 / 1_000_000.0);

                open_chrome(&tabs);
            }
            Err(e) => println!("Decode json Err: {}", e),
        }
    }

    Ok(())
    
}
