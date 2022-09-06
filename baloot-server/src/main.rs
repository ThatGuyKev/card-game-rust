use anyhow::Result;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
    future::{self, join},
    lock::Mutex,
    StreamExt,
};
use smol::{Async, Executor};
use std::{
    collections::HashMap,
    io::Error as IoError,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::Arc,
};

use baloot_common::{Card, ClientMessage, Game};
use tungstenite::Message as WebsocketMessage;

struct Player {
    name: String,
    hand: HashMap<Card, usize>,
    ws: Option<UnboundedSender<String>>,
}
struct Team {
    name: String,
    score: u32,
    players: Vec<Player>,
}
#[derive(Default)]
struct Room {
    name: String,
    started: bool,
    ended: bool,
    connections: HashMap<SocketAddr, usize>,
    players: Vec<Player>,
    active_player: usize,
    game: Game,
}
impl Room {
    fn on_message(&mut self, addr: SocketAddr, msg: ClientMessage) -> bool {
        match msg {
            ClientMessage::Chat(c) => {
                let name = self
                    .connections
                    .get(&addr)
                    .map_or("unknown", |i| &self.players[*i].name);
                println!("chat received!")
            }
            msg => println!("something came, {:?}", msg),
        }
        true
    }
}
type TaggedClientMessage = (SocketAddr, ClientMessage);

#[derive(Clone)]
struct RoomHandle {
    write: UnboundedSender<TaggedClientMessage>,
    room: Arc<Mutex<Room>>,
}

impl RoomHandle {
    async fn run_room(&mut self, mut read: UnboundedReceiver<TaggedClientMessage>) {
        while let Some((addr, msg)) = read.next().await {
            if !self.room.lock().await.on_message(addr, msg) {
                println!("something went wrong");
                break;
            }
        }
    }
}
type RoomList = Arc<Mutex<HashMap<String, RoomHandle>>>;

async fn handle_connection(
    rooms: RoomList,
    mut raw_stream: Async<TcpStream>,
    addr: SocketAddr,
) -> Result<()> {
    println!("[{}] Incoming TCP connection", addr);
    // let mut buf = vec![];

    // raw_stream.read_to_end(&mut buf)?;

    // let mut ws_stream = tokio_tungstenite::accept_async(raw_stream).await.unwrap();
    // while let Ok(t) = ws_stream.next().await {}
    // println!("[{}] WebSocket connection established", addr);
    // let msg = bincode::deserialize::<ClientMessage>(&buf).unwrap();
    // println!("[{}] Received message {:?}", addr, msg);

    // match msg {
    //     ClientMessage::CreateRoom(player_name) => {
    //         println!("[{}] Welcome {} to 'the creepy room :)'", addr, player_name)
    //     }
    //     msg => {
    //         println!("[{}] Unknown message received {:?}", addr, msg);
    //     }
    // }

    let mut ws_stream = async_tungstenite::accept_async(raw_stream).await?;
    println!("[{}] WebSocket connection established", addr);
    while let Some(Ok(WebsocketMessage::Binary(t))) = ws_stream.next().await {
        println!("{:?}", t);
        let msg = bincode::deserialize::<ClientMessage>(&t)?;
        println!("[{}] Received message {:?}", addr, msg);

        match msg {
            ClientMessage::CreateRoom(player_name) => {
                let (write, read) = unbounded();
                let room = Arc::new(Mutex::new(Room::default()));
                let handle = RoomHandle { write, room };

                let room_name = {
                    let map = &mut rooms.lock().await;
                    "untamed monsters".to_string()
                };
                println!("[{}] Welcome {} to room {}", addr, player_name, room_name);

                handle.room.lock().await.name = room_name.clone();

                let mut h = handle.clone();
                h.run_room(read).await;
            }
            msg => {
                println!("[{}] Unknown message received {:?}", addr, msg);
            }
        }
    }

    Ok(())
}

fn main() -> Result<(), IoError> {
    // let ex = Executor::new();

    let addr = "0.0.0.0:8080";

    let rooms = RoomList::new(Mutex::new(HashMap::new()));
    // std::thread::spawn(move || smol::future::block_on(ex.run(future::pending::<()>())));

    smol::future::block_on(async {
        println!("Listening on: {}", addr);
        // let listener = TcpListener::bind(addr).unwrap();

        let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 8080))
            .expect("Could not establish listener");

        while let Ok((stream, addr)) = listener.accept().await {
            println!("{}", addr);
            let rooms = rooms.clone();

            smol::spawn(async move {
                println!("handling connection");
                if let Err(e) = handle_connection(rooms, stream, addr).await {
                    println!("Failed to handle connection from {}: {}", addr, e)
                }
            })
            .detach()
        }
    });

    Ok(())
}
