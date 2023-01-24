use protohackers_rs::run_server;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tracing::{debug, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    run_server(8000, handle_client).await?;
    Ok(())
}

async fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    let (input_stream, output_stream) = stream.split();
    handle_client_internal(input_stream, output_stream).await
}

async fn handle_client_internal(
    input_stream: impl AsyncRead + Unpin,
    mut output_stream: impl AsyncWrite + Unpin,
) -> anyhow::Result<()> {
    let input_stream = BufReader::new(input_stream);
    let mut lines = input_stream.lines();
    while let Some(line) = lines.next_line().await? {
        debug!("Got a line {}", line);
        match serde_json::from_str::<Request>(&line) {
            Ok(req) => {
                let response = Response::new(is_prime(req.number));
                let mut bytes = serde_json::to_vec(&response)?;
                bytes.push(b'\n');
                output_stream.write_all(&bytes).await?;
                output_stream.flush().await?;
            }
            Err(e) => {
                error!("Malformed request {}", e);
                output_stream.write_all(b"malformed\n").await?;
                output_stream.flush().await?;
                break;
            }
        };
    }
    Ok(())
}

fn is_prime(n: f64) -> bool {
    if n < 2_f64 || n.trunc() != n {
        return false;
    };

    let n_int = n.trunc() as u64;
    if n_int <= 3 {
        return true;
    }

    (2..=(n.sqrt() as u64)).all(|a| (n_int) % a != 0)
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Request {
    method: Method,
    number: f64,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    IsPrime,
}

#[derive(Serialize, Debug)]
pub struct Response {
    method: Method,
    prime: bool,
}

impl Response {
    pub fn new(prime: bool) -> Self {
        Self {
            method: Method::IsPrime,
            prime,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::handle_client_internal;

    #[tokio::test]
    async fn prime_time_test_malformed() {
        let input = "{}\n";
        let mut output: Vec<u8> = vec![];

        handle_client_internal(input.as_bytes(), &mut output)
            .await
            .expect("Failed to handle");

        assert_eq!(
            String::from("malformed\n"),
            String::from_utf8(output).unwrap()
        );
    }

    #[tokio::test]
    async fn prime_time_test_ok() {
        let input = serde_json::json!({"method": "isPrime", "number": 3});
        let input: Vec<u8> = serde_json::to_vec(&input).unwrap();

        let mut output: Vec<u8> = vec![];

        handle_client_internal(input.as_slice(), &mut output)
            .await
            .expect("Failed to handle");

        let response = String::from_utf8(output).unwrap();
        let json: serde_json::Value = serde_json::from_str(&response.trim()).unwrap();
        assert_eq!(
            serde_json::json!({"method": "isPrime", "prime": true}),
            json
        );
    }
}
