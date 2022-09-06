use serde::{Deserialize, Serialize};
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub enum Suit {
    Spades,
    Diamonds,
    Clubs,
    Hearts,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
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

#[derive(Debug, Deserialize, Serialize)]
pub enum Calling {
    Hokom,
    Sun,
    SecondHokom,
    Ashka,
    Pass,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessage {
    CreateRoom(String),
    JoinedRoom(String, String),
    Chat(String),
    Play(Card),
    Disconnected,
    Declare(Calling, Suit),
}
#[derive(Debug, Deserialize, Serialize)]
pub enum ServerMessage {
    JoinedRoom {
        room_name: String,
        players: Vec<(String, u32, bool)>,
        active_player: usize,
        player_index: usize,
        deck: Vec<Card>,
    },
    JoinFailed(String),

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