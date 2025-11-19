mod utils;

use std::env;

use tokio::net::{TcpStream};
use tokio::io::{AsyncReadExt};
use utils::chrome::open_chrome;
use dotenv::dotenv;

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    dotenv().ok();

    let addr = format!("{}:24811" , env::var("PORT").unwrap());
    let mut stream = TcpStream::connect(&addr).await?;
    let mut buffer = [0u8; 1024];
    println!("server started");
    

    loop {
        let n = stream.read(&mut buffer).await?;

        if n == 0 {
            println!("connection closed");
        }

        let msg = String::from_utf8_lossy(&buffer[..n]);
        println!("Received : {}" , msg);

        let urls: Vec<String> = serde_json::from_str(&msg)?;
        if !urls.is_empty() {
            open_chrome(urls);
        } 
    }
    
}
