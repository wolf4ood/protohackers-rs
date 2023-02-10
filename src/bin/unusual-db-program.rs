use std::collections::HashMap;

use tokio::net::UdpSocket;
use tracing::info;

static EMPTY: String = String::new();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Unusual Database service on 8000 port");

    let socket = UdpSocket::bind("localhost:8000").await?;

    let mut store = HashMap::new();
    loop {
        let mut buff = [0; 1000];

        let (read, addr) = socket.recv_from(&mut buff).await?;

        let msg = std::str::from_utf8(&buff[0..read])?;

        match msg.split_once("=") {
            Some((prefix, suffix)) => {
                store.insert(prefix.to_string(), suffix.to_string());
            }
            None => {
                let value = store.get(msg).unwrap_or(&EMPTY);

                let response = format!("{}={}", msg, value);

                socket.send_to(response.as_bytes(), addr).await?;
            }
        };
    }
}
