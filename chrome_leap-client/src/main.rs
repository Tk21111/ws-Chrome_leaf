use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};


#[tokio::main]
async fn main() -> anyhow::Result<()> {

    let mut stream = TcpStream::connect("127.0.0.1:24811").await?;
    let mut buffer = [0u8; 1024];
    println!("server started");


    loop {
        let n = stream.read(&mut buffer).await?;

        if n == 0 {
            println!("connection closed");
        }

        let msg = String::from_utf8_lossy(&buffer[..n]);
        println!("Received : {}" , msg);
    }
    
}
