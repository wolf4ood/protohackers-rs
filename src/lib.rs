use std::{future::Future, net::SocketAddr};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

pub async fn run_server<H, F>(port: u16, handler: H) -> anyhow::Result<()>
where
    H: Fn(TcpStream) -> F,
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    run_server_with_state(port, (), |_, stream, _| handler(stream)).await
}

pub async fn run_server_with_state<H, S, F>(port: u16, state: S, handler: H) -> anyhow::Result<()>
where
    S: Clone,
    H: Fn(S, TcpStream, SocketAddr) -> F,
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let listener = TcpListener::bind(&format!("0.0.0.0:{}", port)).await?;

    info!("Starting server at 0.0.0.0:{}", port);
    loop {
        let (socket, address) = listener.accept().await?;

        debug!("Got connection from {}", address);
        let future = handler(state.clone(), socket, address);
        tokio::task::spawn(async move {
            if let Err(err) = future.await {
                error!("Error handling connection {}: {}", address, err);
            }
        });
    }
}
