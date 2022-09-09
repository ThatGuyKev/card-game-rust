use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Blob;
use web_sys::FileReader;
use web_sys::MessageEvent;
use web_sys::ProgressEvent;

use std::sync::Mutex;
use web_sys::{
    Document, Element, Event, EventTarget, HtmlButtonElement, HtmlElement, HtmlInputElement,
    KeyboardEvent, SvgGraphicsElement, WebSocket,
};

use baloot_common::{Calling, Card, ClientMessage, ServerMessage, Suit};

macro_rules! console_log {
    ($($t:tt)*) => (web_sys::console::log_1(&format!($($t)*).into()))
}

type JsResult<T> = Result<T, JsValue>;
type JsError = Result<(), JsValue>;
type JsClosure<T> = Closure<dyn FnMut(T) -> JsError>;

macro_rules! methods {
    ($($sub:ident => [$($name:ident($($var:ident: $type:ty),*)),+ $(,)?]),+
       $(,)?) =>
    {
        $($(
        fn $name(&mut self, $($var: $type),* ) -> JsError {
            match self {
                State::$sub(s) => s.$name($($var),*),
                _ => panic!("Invalid state transition"),
            }
        }
        )+)+
    }
}

macro_rules! transitions {
    ($($sub:ident => [$($name:ident($($var:ident: $type:ty),*)
                        -> $into:ident),+ $(,)?]),+$(,)?) =>
    {
        $($(
        fn $name(&mut self, $($var: $type),* ) -> JsError {
            let s = std::mem::replace(self, State::Empty);
            match s {
                State::$sub(s) => *self = State::$into(s.$name($($var),*)?),
                _ => panic!("Invalid state"),
            }
            Ok(())
        }
        )+)+
    }
}

enum TableState {
    Idle,
    // Declaring(Declaring),
    // Dealing(Card),
    // Animation(String),
}

pub struct Table {
    doc: Document,
    svg: SvgGraphicsElement,
    svg_div: Element,
    tentative_score_span: Option<HtmlElement>,
    state: TableState,

    my_turn: bool,
    hand: Vec<Card>,
    declaration: Calling,
    top_suit: Suit,
    cards_played: Vec<Card>,
}
impl Table {
    fn new(doc: &Document) -> JsResult<Table> {
        let svg = doc
            .get_element_by_id("game_svg")
            .expect("could not get game_svg")
            .dyn_into()?;
        let svg_div = doc
            .get_element_by_id("svg_div")
            .expect("could not get svg_div")
            .dyn_into()?;

        let out = Table {
            doc: doc.clone(),
            state: TableState::Idle,
            svg,
            svg_div,
            tentative_score_span: None,
            declaration: Calling::Pass,
            top_suit: Suit::Clubs,
            hand: Vec::new(),
            my_turn: false,
            cards_played: Vec::new(),
        };
        Ok(out)
    }
}
pub struct Base {
    doc: Document,
    ws: WebSocket,
}
impl Base {
    fn send(&self, msg: ClientMessage) -> JsError {
        let encoded = bincode::serialize(&msg)
            .map_err(|e| JsValue::from_str(&format!("could not encode: {}", e)))?;
        self.ws.send_with_u8_array(&encoded[..])
    }
}
struct Connecting {
    base: Base,
}
impl Connecting {
    fn on_connected(self) -> JsResult<CreateOrJoin> {
        self.base
            .doc
            .get_element_by_id("disconnected_msg")
            .expect("could not get 'disconnected_msg'")
            .dyn_into::<HtmlElement>()?
            .set_text_content(Some("lost connection to game server"));
        CreateOrJoin::new(self.base)
    }
}
struct CreateOrJoin {
    base: Base,
    name_input: HtmlInputElement,
    play_button: HtmlButtonElement,
    _submit_cb: JsClosure<Event>,
}
impl CreateOrJoin {
    fn new(base: Base) -> JsResult<CreateOrJoin> {
        let name_input = base
            .doc
            .get_element_by_id("name_input")
            .expect("could not find name_input")
            .dyn_into::<HtmlInputElement>()?;

        let form = base
            .doc
            .get_element_by_id("join_form")
            .expect("could not find join_form");

        let submit_cb = set_event_cb(&form, "submit", move |e: Event| {
            e.prevent_default();
            HANDLE.lock().unwrap().on_join_button()
        });
        let play_button = base
            .doc
            .get_element_by_id("play_button")
            .expect("could not find play_button")
            .dyn_into::<HtmlButtonElement>()?;
        play_button.set_text_content(Some("Create new room"));
        play_button.class_list().remove_1("disabled")?;

        Ok(CreateOrJoin {
            base,
            name_input,
            play_button,
            _submit_cb: submit_cb,
        })
    }
    fn on_join_button(&self) -> JsError {
        self.play_button.set_disabled(true);
        let name = self.name_input.value();

        let msg = ClientMessage::CreateRoom(name);
        self.base.send(msg)
    }

    fn on_joined_room(
        self,
        room_name: &str,
        players: &[(String, bool)],
        active_player: usize,
        player_index: usize,
    ) -> JsResult<Playing> {
        console_log!("joined room");

        self.base
            .doc
            .get_element_by_id("join")
            .expect("could not get join")
            .dyn_into::<HtmlElement>()?
            .set_hidden(true);
        let mut p = Playing::new(self.base, room_name, players, active_player, player_index)?;
        p.on_information("welcome")?;
        console_log!("joined room");

        Ok(p)
    }
}

struct Playing {
    base: Base,

    table: Table,
    score_table: HtmlElement,
    chat_div: HtmlElement,
    chat_input: HtmlInputElement,
    player_index: usize,
    active_player: usize,
    player_names: Vec<String>,
    _keyup_cb: JsClosure<KeyboardEvent>,
}

impl Playing {
    fn new(
        base: Base,
        room_name: &str,
        players: &[(String, bool)],
        active_player: usize,
        player_index: usize,
    ) -> JsResult<Playing> {
        let s: HtmlElement = base
            .doc
            .get_element_by_id("room_name")
            .expect("Could not get room_name")
            .dyn_into()?;
        s.set_text_content(Some(room_name));

        let chat_input = base
            .doc
            .get_element_by_id("chat_input")
            .expect("could not get chat_input")
            .dyn_into()?;
        let chat_div = base
            .doc
            .get_element_by_id("chat_msgs")
            .expect("could not get chat_msgs")
            .dyn_into()?;

        let keyup_cb = set_event_cb(&chat_input, "keyup", move |e: KeyboardEvent| {
            if e.key_code() == 13 {
                e.prevent_default();
                console_log!("sending message..");
                HANDLE.lock().unwrap().on_send_chat()
            } else {
                Ok(())
            }
        });
        let table = Table::new(&base.doc)?;

        let score_table = base
            .doc
            .get_element_by_id("score_rows")
            .expect("Could not get score_rows")
            .dyn_into()?;

        let mut out = Playing {
            base,
            table,

            score_table,
            chat_input,
            chat_div,
            player_index,
            active_player,
            player_names: Vec::new(),
            _keyup_cb: keyup_cb,
        };
        // let s = out
        //     .score_table
        //     .child_nodes()
        //     .item(out.player_index as u32 + 3)
        //     .expect("Could not get table row")
        //     .child_nodes()
        //     .item(2)
        //     .expect("Could not get score value")
        //     .child_nodes()
        //     .item(1)
        //     .expect("Could not get second span")
        //     .dyn_into::<HtmlElement>()?;

        // out.table.tentative_score_span = Some(s);
        Ok(out)
    }

    fn on_chat(&self, from: &str, msg: &str) -> JsError {
        let p = self.base.doc.create_element("p")?;
        p.set_class_name("msg");

        let b = self.base.doc.create_element("b")?;
        b.set_text_content(Some(from));
        p.append_child(&b)?;

        let s = self.base.doc.create_element("span")?;
        s.set_text_content(Some(msg));
        p.append_child(&s)?;

        self.chat_div.append_child(&p)?;
        self.chat_div.set_scroll_top(self.chat_div.scroll_height());
        Ok(())
    }

    fn on_send_chat(&self) -> JsError {
        let input_value = self.chat_input.value();
        console_log!("sending message {}", input_value);
        if !input_value.is_empty() {
            self.chat_input.set_value("");
            self.base.send(ClientMessage::Chat(input_value))
        } else {
            Ok(())
        }
    }

    fn on_information(&self, msg: &str) -> JsError {
        let p = self.base.doc.create_element("p")?;
        p.set_class_name("msg");

        let i = self.base.doc.create_element("i")?;
        i.set_text_content(Some(msg));
        p.append_child(&i)?;
        self.chat_div.append_child(&p)?;
        self.chat_div.set_scroll_top(self.chat_div.scroll_height());
        Ok(())
    }
}
enum State {
    Connecting(Connecting),
    CreateOrJoin(CreateOrJoin),
    // Declaring(Declaring),
    Playing(Playing),
    Empty,
}

impl State {
    transitions!(
        Connecting =>[
            on_connected() -> CreateOrJoin,
        ],
        CreateOrJoin => [
            on_joined_room(room_name: &str, players: &[(String,bool)],active_player:usize, player_index: usize) -> Playing
        ],


    );
    methods!(
        Playing => [
            on_chat(from:&str, msg:&str),
            on_information(msg: &str),
            on_send_chat()
        ],
        CreateOrJoin => [
            on_join_button(),
        ]

    );
}

unsafe impl Send for State {}

lazy_static::lazy_static! {
    static ref HANDLE: Mutex<State> = Mutex::new(State::Empty);

}

#[must_use]
fn build_cb<F, T>(f: F) -> JsClosure<T>
where
    F: FnMut(T) -> JsError + 'static,
    T: FromWasmAbi + 'static,
{
    Closure::wrap(Box::new(f) as Box<dyn FnMut(T) -> JsError>)
}

#[must_use]
fn set_event_cb<E, F, T>(obj: &E, name: &str, f: F) -> JsClosure<T>
where
    E: JsCast + Clone + std::fmt::Debug,
    F: FnMut(T) -> JsError + 'static,
    T: FromWasmAbi + 'static,
{
    let cb = build_cb(f);
    let target = obj
        .dyn_ref::<EventTarget>()
        .expect("Could not convert into `EventTarget`");
    target
        .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())
        .expect("Could not add event listener");
    cb
}

fn on_message(msg: ServerMessage) -> JsError {
    use ServerMessage::*;
    let mut state = HANDLE.lock().unwrap();

    match msg {
        JoinedRoom {
            room_name,
            players,
            active_player,
            player_index,
        } => state.on_joined_room(&room_name, &players, active_player, player_index),
        Chat { from, message } => state.on_chat(&from, &message),
        Information(message) => state.on_information(&message),
        msg => Ok(()),
    }
}

#[wasm_bindgen(start)]
pub fn main() -> JsError {
    console_error_panic_hook::set_once();
    console_log!("starting baloot!");

    let hostname = format!("ws://localhost:8080");

    let doc = web_sys::window()
        .expect("no global 'window' exists")
        .document()
        .expect("no doc in window");
    console_log!("connexting to websocket at {}", hostname);

    let ws = WebSocket::new(&hostname)?;

    set_event_cb(&ws, "open", move |_: JsValue| {
        HANDLE.lock().unwrap().on_connected()
    })
    .forget();

    let on_decoded_cb = Closure::wrap(Box::new(move |e: ProgressEvent| {
        let target = e.target().expect("could not get target");
        let reader: FileReader = target.dyn_into().expect("could not cast");
        let result = reader.result().expect("could not get result");
        let buf = js_sys::Uint8Array::new(&result);
        let mut data = vec![0; buf.length() as usize];
        buf.copy_to(&mut data[..]);
        let msg = bincode::deserialize(&data[..])
            .map_err(|e| JsValue::from_str(&format!("failed to deserialize {}", e)))
            .expect("could not decode message");
        on_message(msg).expect("message matching failed")
    }) as Box<dyn FnMut(ProgressEvent)>);

    set_event_cb(&ws, "message", move |e: MessageEvent| {
        let blob = e.data().dyn_into::<Blob>()?;
        let fr = FileReader::new()?;
        fr.add_event_listener_with_callback("load", on_decoded_cb.as_ref().unchecked_ref())?;

        fr.read_as_array_buffer(&blob)?;
        Ok(())
    })
    .forget();

    set_event_cb(&ws, "close", move |_: Event| -> JsError {
        let doc = web_sys::window()
            .expect("no global `window` exists")
            .document()
            .expect("should have a document on window");
        for d in ["join", "playing"].iter() {
            doc.get_element_by_id(d)
                .expect("Could not get major div")
                .dyn_into::<HtmlElement>()?
                .set_hidden(true);
        }
        doc.get_element_by_id("disconnected")
            .expect("Could not get disconnected div")
            .dyn_into::<HtmlElement>()?
            .set_hidden(false);
        Ok(())
    })
    .forget();
    let base = Base { doc, ws };

    base.doc
        .get_element_by_id("play_button")
        .expect("could not get play_button")
        .dyn_into::<HtmlElement>()?
        .set_text_content(Some("Connecting.."));

    *HANDLE.lock().unwrap() = State::Connecting(Connecting { base });
    Ok(())
}
