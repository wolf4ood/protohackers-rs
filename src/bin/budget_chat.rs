use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use futures::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use protohackers_rs::run_server_with_state;
use tokio::{
    net::TcpStream,
    sync::{mpsc, Mutex},
};
use tokio_util::codec::{Decoder, Encoder, Framed, LinesCodec};
use tracing::error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    run_server_with_state(8000, Room::new(), handle_client).await?;
    Ok(())
}

async fn handle_client(state: Room, stream: TcpStream, address: SocketAddr) -> anyhow::Result<()> {
    let (input_stream, output_stream) = Framed::new(stream, ChatCodec::new()).split();

    handle_client_internal(state, address, input_stream, output_stream).await
}

async fn handle_client_internal<I, O>(
    state: Room,
    address: SocketAddr,
    mut sink: O,
    mut stream: I,
) -> anyhow::Result<()>
where
    I: Stream<Item = anyhow::Result<String>> + Unpin,
    O: Sink<OutgoingMessage, Error = anyhow::Error> + Unpin,
{
    sink.send(OutgoingMessage::Welcome).await?;
    let username = stream
        .try_next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Error while waiting for the username"))?;

    let username = match Username::parse(username) {
        Ok(username) => username,
        Err(e) => {
            sink.send(OutgoingMessage::InvalidUsername(e.to_string()))
                .await?;
            return Ok(());
        }
    };
    let mut handle = state.join(address, username).await;

    loop {
        tokio::select! {
            Some(msg) = handle.receiver.recv() => {

                if let Err(e) = sink.send(msg).await {
                    error!("Error sending message {}",e);
                    break;
                }
            }

            result = stream.next() => match result {
                Some(Ok(msg)) =>  {
                    handle.send_message(msg).await;
                }
                Some(Err(e)) => {

                    error!("Error reading messages {}",e);
                    break;
                }
                None => break,
            }
        };
    }

    handle.leave().await;
    Ok(())
}

pub struct ChatCodec {
    lines: LinesCodec,
}

impl ChatCodec {
    pub fn new() -> Self {
        Self {
            lines: LinesCodec::new(),
        }
    }
}

impl Encoder<OutgoingMessage> for ChatCodec {
    type Error = anyhow::Error;

    fn encode(
        &mut self,
        item: OutgoingMessage,
        dst: &mut bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        self.lines
            .encode(item.to_string(), dst)
            .map_err(anyhow::Error::from)
    }
}
impl Decoder for ChatCodec {
    type Item = String;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.lines.decode(src).map_err(anyhow::Error::from)
    }
}

#[derive(Clone, derive_more::Display, PartialEq, Debug)]
pub struct Username(String);

impl Username {
    pub fn parse(input: String) -> anyhow::Result<Username> {
        if input.trim().is_empty() {
            anyhow::bail!("Name should be at least 1 character")
        }
        if input.trim().chars().any(|c| !c.is_alphanumeric()) {
            anyhow::bail!("Name should contains only alphanumeric characters")
        }
        Ok(Username(input))
    }

    pub fn inner_ref(&self) -> &String {
        &self.0
    }
}

#[derive(derive_more::Display, Clone, PartialEq, Debug)]
pub enum OutgoingMessage {
    #[display(fmt = "Welcome to budgetchat! What shall I call you?")]
    Welcome,
    #[display(fmt = "* {} has entered the room", _0)]
    Join(Username),
    #[display(fmt = "* {} has left the room", _0)]
    Leave(Username),
    #[display(fmt = "[{}] {}", from, msg)]
    Chat { from: Username, msg: String },
    #[display(fmt = "Invalid username {}", _0)]
    InvalidUsername(String),
    #[display(fmt = "* The room contains: {}", "self.participants(_0)")]
    Participants(Vec<Username>),
}

impl OutgoingMessage {
    fn participants(&self, participants: &[Username]) -> String {
        participants
            .iter()
            .map(|user| user.to_string())
            .collect::<Vec<String>>()
            .join(", ")
    }
}

pub struct Peer {
    username: Username,
    sender: mpsc::UnboundedSender<OutgoingMessage>,
}

pub struct PeerHandle {
    username: Username,
    address: SocketAddr,
    receiver: mpsc::UnboundedReceiver<OutgoingMessage>,
    room: Room,
}

impl PeerHandle {
    pub async fn send_message(&self, msg: String) {
        self.room
            .broadcast(
                &self.address,
                OutgoingMessage::Chat {
                    from: self.username.clone(),
                    msg,
                },
            )
            .await;
    }

    pub async fn leave(self) {
        self.room.leave(&self.address).await
    }
}

#[derive(Clone)]
pub struct Room(Arc<Mutex<HashMap<SocketAddr, Peer>>>);

impl Room {
    pub fn new() -> Room {
        Room(Arc::new(Mutex::new(HashMap::new())))
    }

    pub async fn join(&self, addr: SocketAddr, username: Username) -> PeerHandle {
        let mut users = self.0.lock().await;
        let (sender, receiver) = mpsc::unbounded_channel();

        let names = users
            .iter()
            .map(|(_, peer)| peer.username.clone())
            .collect::<Vec<Username>>();

        let _ = sender.send(OutgoingMessage::Participants(names));

        self.broadcast_internal(&addr, OutgoingMessage::Join(username.clone()), &mut users)
            .await;
        users.insert(
            addr,
            Peer {
                username: username.clone(),
                sender,
            },
        );

        PeerHandle {
            username,
            receiver,
            room: self.clone(),
            address: addr,
        }
    }

    async fn leave(&self, addr: &SocketAddr) {
        let mut users = self.0.lock().await;
        if let Some(leaving) = users.remove(addr) {
            self.broadcast_internal(addr, OutgoingMessage::Leave(leaving.username), &mut users)
                .await;
        }
    }

    async fn broadcast_internal(
        &self,
        addr: &SocketAddr,
        msg: OutgoingMessage,
        users: &mut HashMap<SocketAddr, Peer>,
    ) {
        for (peer_addr, peer) in users.iter_mut() {
            if addr != peer_addr {
                let _ = peer.sender.send(msg.clone());
            }
        }
    }

    async fn broadcast(&self, addr: &SocketAddr, msg: OutgoingMessage) {
        let mut users = self.0.lock().await;
        self.broadcast_internal(addr, msg, &mut users).await;
    }
}

#[cfg(test)]
mod budget_chat_tests {
    use std::net::SocketAddr;

    use futures::SinkExt;
    use tokio::{
        sync::mpsc::{self, Receiver, Sender},
        task::JoinHandle,
    };
    use tokio_util::sync::PollSender;

    use crate::{handle_client_internal, OutgoingMessage, PeerHandle, Room, Username};

    async fn check_message(handle: &mut PeerHandle, msg: OutgoingMessage) {
        assert_eq!(handle.receiver.recv().await.unwrap(), msg);
    }

    #[tokio::test]
    async fn room_test() {
        let room = Room::new();
        let alice_username = Username::parse("alice".to_string()).unwrap();
        let bob_username = Username::parse("bob".to_string()).unwrap();

        let mut alice = room
            .join("0.0.0.0:100".parse().unwrap(), alice_username.clone())
            .await;

        // alice should receive an empty participants list on join
        check_message(&mut alice, OutgoingMessage::Participants(vec![])).await;

        let mut bob = room
            .join("0.0.0.0:200".parse().unwrap(), bob_username.clone())
            .await;

        // bob should receive participants list on join with only alice
        check_message(
            &mut bob,
            OutgoingMessage::Participants(vec![alice_username.clone()]),
        )
        .await;

        // alice should receive bob join notification
        check_message(&mut alice, OutgoingMessage::Join(bob_username.clone())).await;

        alice.send_message("Hi Bob".to_string()).await;

        // bob should receive alice message
        check_message(
            &mut bob,
            OutgoingMessage::Chat {
                msg: "Hi Bob".to_string(),
                from: alice_username.clone(),
            },
        )
        .await;

        alice.leave().await;

        // bob should receive alice left notification
        check_message(&mut bob, OutgoingMessage::Leave(alice_username.clone())).await;
    }

    struct PeerTest {
        sink_receiver: Receiver<OutgoingMessage>,
        stream_sender: Option<Sender<anyhow::Result<String>>>,
        handle: JoinHandle<anyhow::Result<()>>,
    }

    impl PeerTest {
        async fn send(&mut self, message: &str) {
            self.stream_sender
                .as_ref()
                .unwrap()
                .send(Ok(message.to_string()))
                .await
                .unwrap();
        }

        async fn leave(mut self) {
            let stream = self.stream_sender.take();
            drop(stream);

            self.handle.await.unwrap().unwrap()
        }

        async fn check_message(&mut self, msg: OutgoingMessage) {
            assert_eq!(self.sink_receiver.recv().await.unwrap(), msg);
        }
    }

    async fn connect(room: Room, addr: &str) -> PeerTest {
        let (sink_tx, sink_rx) = mpsc::channel(100);
        let (stream_tx, mut stream_rx) = mpsc::channel(100);

        let address: SocketAddr = addr.parse().unwrap();

        let stream = async_stream::stream! {
            while let Some(message) = stream_rx.recv().await {
                yield message
            }
        };

        let handle = tokio::spawn(async move {
            handle_client_internal(
                room,
                address,
                PollSender::new(sink_tx).sink_map_err(anyhow::Error::from),
                Box::pin(stream),
            )
            .await
        });

        PeerTest {
            sink_receiver: sink_rx,
            stream_sender: Some(stream_tx),
            handle,
        }
    }

    #[tokio::test]
    async fn example_session_test() {
        let room = Room::new();

        let alice_username = Username::parse("alice".to_string()).unwrap();
        let bob_username = Username::parse("bob".to_string()).unwrap();

        let mut alice = connect(room.clone(), "0.0.0.0:10").await;
        alice.check_message(OutgoingMessage::Welcome).await;

        alice.send(&alice_username.inner_ref()).await;
        alice
            .check_message(OutgoingMessage::Participants(vec![]))
            .await;

        let mut bob = connect(room.clone(), "0.0.0.0:11").await;
        bob.check_message(OutgoingMessage::Welcome).await;

        bob.send(&bob_username.inner_ref()).await;
        bob.check_message(OutgoingMessage::Participants(vec![alice_username.clone()]))
            .await;

        alice
            .check_message(OutgoingMessage::Join(bob_username.clone()))
            .await;

        alice.send("Hi bob!").await;

        bob.check_message(OutgoingMessage::Chat {
            msg: "Hi bob!".to_string(),
            from: alice_username.clone(),
        })
        .await;

        bob.send("Hi alice!").await;

        alice
            .check_message(OutgoingMessage::Chat {
                msg: "Hi alice!".to_string(),
                from: bob_username.clone(),
            })
            .await;

        bob.leave().await;

        alice
            .check_message(OutgoingMessage::Leave(bob_username))
            .await;
    }
}
