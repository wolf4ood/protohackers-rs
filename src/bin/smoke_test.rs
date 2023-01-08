use bytes::BytesMut;
use protohackers_rs::run_server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    run_server(8000, |mut socket| async move {
        let mut buffer = BytesMut::with_capacity(1024);
        loop {
            let read = socket.read_buf(&mut buffer).await?;
            if read == 0 {
                break;
            }
        }
        socket.write_all(&buffer).await?;
        Ok(())
    })
    .await?;
    Ok(())
}
