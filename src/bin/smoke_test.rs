use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let listener = TcpListener::bind("127.0.0.1:8000").await?;

    info!("Starting Echo service on 8000 port");

    loop {
        let (socket, address) = listener.accept().await?;
        debug!("Got connection from {}", address);
        tokio::task::spawn(async move {
            if let Err(err) = handle_client(socket).await {
                error!("Error handling client connection {}: {}", address, err);
            }
        });
    }
}

async fn handle_client(mut socket: TcpStream) -> anyhow::Result<()> {
    let mut buf = [0; 1024];
    loop {
        let read = socket.read(&mut buf).await?;

        if read == 0 {
            break;
        }
        socket.write_all(&buf[..read]).await?;
    }
    Ok(())
}
