use baloot_common::{Card, ClientMessage, Rank, Suit};
use smol::Async;
use std::io::{Error as IoError, Write};
use std::net::{TcpStream, ToSocketAddrs};
use tungstenite::{connect, Message};

fn main() -> Result<(), IoError> {
    let card: Card = (Rank::Ace, Suit::Spades);
    smol::future::block_on(async {
        let (mut socket, response) = connect("ws://localhost:8080").unwrap();

        println!("Connected to server");
        // let mut stream = TcpStream::connect("0.0.0.0:8080").unwrap();

        let buf = bincode::serialize::<ClientMessage>(&ClientMessage::Play(card)).unwrap();

        socket.write_message(Message::Binary(buf)).unwrap();
    });
    Ok(())
}
