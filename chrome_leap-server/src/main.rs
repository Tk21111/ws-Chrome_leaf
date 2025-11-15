use quinn::{Endpoint, Incoming, ServerConfig};
use std::{error::Error, fs, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    
    let cert_der = fs::read("cert.der")?;
    let key_der = fs::read("key.der")?;

    let server_config = ServerConfig::with_single_cert(
        vec![rustls::pki_types::CertificateDer::from(cert_der)], rustls::pki_types::PrivatePkcs8KeyDer::from(key_der).into(),
    )?;

    let bind_addr = "127.0.0.1:5000".parse()?;
    let endpoint = Endpoint::server(server_config, bind_addr)?;

    println!("listening at {}" , endpoint.local_addr()?);

    while let Some(conn) = endpoint.accept().await {
        println!("connection from {}" , conn.remote_address());

        tokio::spawn(async move {
            if let Err(e) = handle_conn(conn).await {
                println!("connection fail {}" , e );
            }
        });
    }
    Ok(())
}

async fn handle_conn(conn : Incoming) -> Result<() , Box<dyn Error>> {
    let connection = conn.await?;

    while let Ok((mut send , mut recv)) = connection.accept_bi().await {
        tokio::spawn(async move {
            if let Err(e) = handle_stream(send, recv).await {
                println!("Stream failed: {}", e);
            }
        });
    }

    Ok(())
}

async fn handle_stream(mut send: quinn::SendStream, mut recv: quinn::RecvStream) -> Result<(), Box<dyn Error>> {
    let msg = recv.read_to_end(1024).await?;
    let msg_str = String::from_utf8_lossy(&msg);

    println!("Received: {}", msg_str);

    send.write_all(b"Hello from the server!").await?;
    println!("Replied and closed stream.");
    
    Ok(())
}