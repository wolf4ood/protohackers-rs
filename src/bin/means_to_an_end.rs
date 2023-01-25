use std::{collections::BTreeMap, io::ErrorKind, ops::RangeInclusive};

use protohackers_rs::run_server;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};

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
    mut input_stream: impl AsyncRead + Unpin,
    mut output_stream: impl AsyncWrite + Unpin,
) -> anyhow::Result<()> {
    let mut buffer = [0; 9];
    let mut prices = Db::new();
    loop {
        match input_stream.read_exact(&mut buffer).await {
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => {
                return Ok(());
            }
            Err(err) => return Err(err.into()),
            Ok(_) => {}
        }

        match Message::parse(&buffer)? {
            Message::Insert { timestamp, price } => {
                prices.insert(timestamp, price);
            }
            Message::Query { mintime, maxtime } => {
                let mean = prices.mean(mintime..=maxtime);
                output_stream.write_i32(mean).await?;
            }
        }
    }
}

pub struct Db(BTreeMap<i32, i32>);

impl Db {
    pub fn new() -> Db {
        Db(BTreeMap::new())
    }

    pub fn insert(&mut self, timestamp: i32, price: i32) {
        self.0.insert(timestamp, price);
    }

    pub fn mean(&self, range: RangeInclusive<i32>) -> i32 {
        if range.is_empty() {
            return 0;
        };
        let (count, sum) = self
            .0
            .range(range)
            .fold((0, 0_i64), |(count, sum), (_, amount)| {
                (count + 1, sum + *amount as i64)
            });

        if count > 0 {
            (sum / count) as i32
        } else {
            0
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum Message {
    Insert { timestamp: i32, price: i32 },
    Query { mintime: i32, maxtime: i32 },
}

impl Message {
    pub fn parse(buffer: &[u8; 9]) -> anyhow::Result<Self> {
        let op = buffer[0];
        let first = i32::from_be_bytes(buffer[1..5].try_into()?);
        let second = i32::from_be_bytes(buffer[5..9].try_into()?);

        match op {
            b'I' => Ok(Message::Insert {
                timestamp: first,
                price: second,
            }),
            b'Q' => Ok(Message::Query {
                mintime: first,
                maxtime: second,
            }),
            _ => Err(anyhow::anyhow!("Unexpected op code {}", op)),
        }
    }
}

#[cfg(test)]
mod means_to_an_end_tests {

    use crate::{handle_client_internal, Message};
    use tokio::io::AsyncWriteExt;

    async fn create_message(op: u8, first: i32, second: i32) -> [u8; 9] {
        let mut buffer = vec![];
        buffer.write_u8(op).await.unwrap();
        buffer.write_i32(first).await.unwrap();
        buffer.write_i32(second).await.unwrap();

        buffer.try_into().unwrap()
    }

    #[tokio::test]
    async fn message_parse_test_insert_ok() {
        let buffer = create_message(b'I', 10, 100).await;

        let message = Message::parse(&buffer).unwrap();

        assert_eq!(
            Message::Insert {
                timestamp: 10,
                price: 100
            },
            message
        )
    }

    #[tokio::test]
    async fn message_parse_test_query_ok() {
        let buffer = create_message(b'Q', 10, 100).await;

        let message = Message::parse(&buffer).unwrap();

        assert_eq!(
            Message::Query {
                mintime: 10,
                maxtime: 100,
            },
            message
        )
    }

    #[tokio::test]
    async fn message_parse_test_fail() {
        let buffer = create_message(b'Z', 10, 100).await;

        let result = Message::parse(&buffer);

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn example_session_test() {
        let messages = vec![
            create_message(b'I', 12345, 101).await,
            create_message(b'I', 123456, 102).await,
            create_message(b'I', 123456, 100).await,
            create_message(b'I', 40960, 5).await,
            create_message(b'Q', 12288, 16384).await,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<u8>>();

        let mut output = vec![];

        handle_client_internal(messages.as_slice(), &mut output)
            .await
            .unwrap();

        assert_eq!(4, output.len());

        assert_eq!(101, i32::from_be_bytes(output[..4].try_into().unwrap()));
    }
}
