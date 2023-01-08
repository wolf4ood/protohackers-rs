use std::future::Future;

use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info};

pub async fn run_server<H, F>(port: u16, handler: H) -> anyhow::Result<()>
where
    H: Fn(TcpStream) -> F,
    F: Future<Output = anyhow::Result<()>> + Send + Sync + 'static,
{
    let listener = TcpListener::bind(&format!("0.0.0.0:{}", port)).await?;

    info!("Starting server at 0.0.0.:{}", port);
    loop {
        let (socket, peer) = listener.accept().await?;

        debug!("Got connection from {}", peer);
        tokio::task::spawn(handler(socket));
    }
}
