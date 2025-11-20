mod utils;

use std::env;
use std::time::Duration;

use tokio::net::{TcpStream};
use tokio::io::{AsyncReadExt};
use utils::chrome::open_chrome;
use dotenv::dotenv;

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

    let mut buffer = [0u8; 1024];
    println!("started");

    loop {
        let n = stream.read(&mut buffer).await?;

        if n == 0 {
            return Err(anyhow::anyhow!("server close connetion"));
        }

        let msg = String::from_utf8_lossy(&buffer[..n]);
        println!("Received : {}" , msg);

        match serde_json::from_str::<Vec<String>>(&msg) {
            Ok(urls) if !urls.is_empty() => {
                open_chrome(&urls);
            }
            Ok(_) => {
                println!("msg empty");
            }
            Err(e) => {
                println!("Decode json Err : {}" , e);
            }
        };
    }
    
}
