use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// use rand::seq::SliceRandom;
// use rand::thread_rng;

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Suit {
    Spades,
    Diamonds,
    Clubs,
    Hearts,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Rank {
    Seven,
    Eight,
    Nine,
    Jack,
    Queen,
    King,
    Ten,
    Ace,
}

pub type Card = (Rank, Suit);

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Declaration {
    Hokom,
    Sun,
    SecondHokom,
    Ashka,
    Pass,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ClientMessage {
    CreateRoom(String),
    JoinRoom(String, String),
    Chat(String),
    Declare(Declaration),
    Play(Card),
    Disconnected,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ServerMessage {
    JoinedRoom {
        room_name: String,
        players: Vec<(String, bool)>,
        active_player: usize,
        player_index: usize,
    },
    JoinFailed(String),
    PlayerTurn(usize),
    Played(Card),
    TeamScore {
        delta: u32,
        total: u32,
    },
    Information(String),
    NewPlayer(String),
    Chat {
        from: String,
        message: String,
    },
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Game {
    pub deck: Vec<Card>,
}
impl Default for Game {
    fn default() -> Game {
        use Rank::*;
        use Suit::*;
        let mut deck = Vec::new();
        for r in &[Seven, Eight, Nine, Jack, Queen, King, Ten, Ace] {
            for s in &[Spades, Hearts, Diamonds, Clubs] {
                deck.push((*r, *s))
            }
        }
        // deck.shuffle(&mut thread_rng());
        Game { deck }
    }
}
impl Game {
    // pub fn shuffle(&mut self) {
    //     self.deck.shuffle(&mut thread_rng());
    // }

    pub fn deal(&mut self, n: usize) -> HashMap<Card, usize> {
        let mut out = HashMap::new();
        for _ in 0..n {
            if let Some(c) = self.deck.pop() {
                *out.entry(c).or_insert(0) += 1;
            }
        }
        out
    }
}
