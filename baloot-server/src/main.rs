use anyhow::Result;
use async_std::net::{SocketAddr, TcpListener, TcpStream};
use async_tungstenite::WebSocketStream;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
    future::join,
    lock::Mutex,
    SinkExt, StreamExt,
};
use rand::Rng;
use std::{collections::HashMap, future, io::Error as IoError, sync::Arc};

use baloot_common::{Card, ClientMessage, Game, ServerMessage};
use tungstenite::Message as WebsocketMessage;

lazy_static::lazy_static! {
    // words.txt is the EFF's random word list for passphrases
    static ref WORD_LIST: Vec<&'static str> = include_str!("words.txt")
        .split('\n')
        .filter(|w| !w.is_empty())
        .collect();
}
#[derive(Debug)]
struct Player {
    name: String,
    hand: HashMap<Card, usize>,
    ws: Option<UnboundedSender<ServerMessage>>,
}
// struct Team {
//     name: String,
//     score: u32,
//     players: Vec<Player>,
// }
#[derive(Default)]
struct Room {
    name: String,
    started: bool,
    // ended: bool,
    connections: HashMap<SocketAddr, usize>,
    players: Vec<Player>,
    active_player: usize,
    game: Game,
}
impl Room {
    fn broadcast(&self, s: ServerMessage) {
        for c in self.connections.values() {
            if let Some(ws) = &self.players[*c].ws {
                if let Err(e) = ws.unbounded_send(s.clone()) {
                    println!(
                        "[{}] Failed to send broadcast to {}: {}",
                        self.name, self.players[*c].name, e
                    );
                }
            }
        }
    }

    fn add_player(
        &mut self,
        addr: SocketAddr,
        player_name: String,
        ws_tx: UnboundedSender<ServerMessage>,
    ) -> Result<()> {
        let hand = self.game.deal(1);

        let mut player_index = None;
        for (i, p) in self.players.iter().enumerate() {
            println!("{} is found {:?}", p.name, p.ws);

            if p.name == player_name {
                player_index = Some(i);
                break;
            }
        }
        if let Some(i) = player_index {
            self.players[i].hand = hand;
            self.players[i].ws = Some(ws_tx.clone());
        } else {
            player_index = Some(self.players.len());

            self.players.push(Player {
                name: player_name,
                hand,
                ws: Some(ws_tx.clone()),
            });
        }

        let player_index = player_index.unwrap();

        self.connections.insert(addr, player_index);

        self.started = true;

        ws_tx.unbounded_send(ServerMessage::JoinedRoom {
            room_name: self.name.clone(),
            players: self
                .players
                .iter()
                .map(|p| (p.name.clone(), p.ws.is_some()))
                .collect(),
            active_player: self.active_player,
            player_index,
        })?;

        Ok(())
    }
    fn on_message(&mut self, addr: SocketAddr, msg: ClientMessage) -> bool {
        match msg {
            ClientMessage::Chat(c) => {
                let name = self
                    .connections
                    .get(&addr)
                    .map_or("unknown", |i| &self.players[*i].name);
                self.broadcast(ServerMessage::Chat {
                    from: name.to_string(),
                    message: c,
                });
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

async fn run_player(
    player_name: String,
    addr: SocketAddr,
    handle: RoomHandle,
    ws_stream: WebSocketStream<TcpStream>,
) {
    let (incoming, outgoing) = ws_stream.split();

    let (ws_tx, ws_rx) = unbounded();

    {
        let room = &mut handle.room.lock().await;
        if let Err(e) = room.add_player(addr, player_name.clone(), ws_tx) {
            println!("[{}] Failed to add player:  {:?}", room.name, e)
        }
    }

    let write = handle.write.clone();
    let ra = ws_rx
        .map(|c| bincode::serialize(&c).unwrap_or_else(|_| panic!("Could not encode {:?}", c)))
        .map(WebsocketMessage::Binary)
        .map(Ok)
        .forward(incoming);

    use bincode::Options;
    let config = bincode::config::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(1024 * 1024);
    let rb = outgoing
        .map(|m| match m {
            Ok(WebsocketMessage::Binary(t)) => config.deserialize::<ClientMessage>(&t).ok(),
            _ => None,
        })
        .take_while(|m| future::ready(m.is_some()))
        .map(|m| m.unwrap())
        .chain(futures::stream::once(async { ClientMessage::Disconnected }))
        .map(move |m| Ok((addr, m)))
        .forward(write);
    let (ra, rb) = join(ra, rb).await;

    if let Err(e) = ra {
        println!(
            "[{}] got error {} from player {} rx queue",
            addr, player_name, e
        );
    }

    if let Err(e) = rb {
        println!(
            "[{}] got error {} from player {} tx queue",
            addr, player_name, e
        );
    }
}

fn next_room_name(rooms: &mut HashMap<String, RoomHandle>, handle: RoomHandle) -> String {
    // This loop should only run once, unless we're starting to saturate the
    // space of possible room names (which is quite large)
    let mut rng = rand::thread_rng();
    loop {
        let room_name = format!(
            "{} {}",
            WORD_LIST[rng.gen_range(0..WORD_LIST.len())],
            WORD_LIST[rng.gen_range(0..WORD_LIST.len())],
        );
        use std::collections::hash_map::Entry;
        if let Entry::Vacant(v) = rooms.entry(room_name.clone()) {
            v.insert(handle);
            return room_name;
        }
    }
}

async fn handle_connection(rooms: RoomList, raw_stream: TcpStream, addr: SocketAddr) -> Result<()> {
    println!("[{}] Incoming TCP connection", addr);

    let mut ws_stream = async_tungstenite::accept_async(raw_stream)
        .await
        .expect("Failed to accept connection");
    println!("[{}] WebSocket connection established", addr);

    while let Some(Ok(WebsocketMessage::Binary(t))) = ws_stream.next().await {
        let msg = bincode::deserialize::<ClientMessage>(&t)?;
        println!("[{}] Received message {:?}", addr, msg);

        match msg {
            ClientMessage::CreateRoom(player_name) => {
                let (write, read) = unbounded();

                let room = Arc::new(Mutex::new(Room::default()));
                let handle = RoomHandle { write, room };
                let room_name = {
                    let map = &mut rooms.lock().await;
                    next_room_name(map, handle.clone())
                };
                println!("[{}] Welcome {} to room {}", addr, player_name, room_name);

                handle.room.lock().await.name = room_name.clone();

                let mut h = handle.clone();

                join(
                    h.run_room(read),
                    run_player(player_name, addr, handle, ws_stream),
                )
                .await;
                println!("[{}] closing room", room_name);
                return Ok(());
            }
            ClientMessage::JoinRoom(name, room_name) => {
                println!("[{}] Player {} sent JoinRoom({})", addr, name, room_name);

                let handle = rooms.lock().await.get_mut(&room_name).cloned();

                if let Some(h) = handle {
                    println!("[{}] player {} joined room {}", addr, name, room_name);

                    run_player(name, addr, h, ws_stream).await;
                    return Ok(());
                } else {
                    println!("[{}] could not find room {}", addr, room_name);

                    let msg = ServerMessage::JoinFailed("Could not find room".to_string());
                    let encoded = bincode::serialize(&msg)?;
                    ws_stream.send(WebsocketMessage::Binary(encoded)).await?;
                }
            }
            msg => {
                println!("[{}] Unknown message received {:?}", addr, msg);
                break;
            }
        }
    }
    println!("[{}] Dropping connection..", addr);
    Ok(())
}

async fn run() {
    {
        let addr = "0.0.0.0:8080";
        let rooms = RoomList::new(Mutex::new(HashMap::new()));

        println!("Listening on: {}", addr);
        // let listener = TcpListener::bind(addr).unwrap();

        let listener = TcpListener::bind(&addr)
            .await
            .expect("Could not establish listener");

        while let Ok((stream, addr)) = listener.accept().await {
            println!("{}", addr);
            let rooms = rooms.clone();

            println!("trying to spawn a task");
            async_std::task::spawn(async move {
                println!("handling connection");
                if let Err(e) = handle_connection(rooms, stream, addr).await {
                    println!("Failed to handle connection from {}: {}", addr, e)
                }
            });
        }
    }
}
fn main() -> Result<(), IoError> {
    async_std::task::block_on(run());

    Ok(())
}
