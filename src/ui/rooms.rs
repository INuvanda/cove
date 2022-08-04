use std::collections::{HashMap, HashSet};
use std::iter;
use std::sync::Arc;

use crossterm::event::KeyCode;
use crossterm::style::{ContentStyle, Stylize};
use parking_lot::FairMutex;
use tokio::sync::mpsc;
use toss::styled::Styled;
use toss::terminal::Terminal;

use crate::euph::api::SessionType;
use crate::euph::{Joined, Status};
use crate::vault::Vault;

use super::input::{key, KeyBindingsList, KeyEvent};
use super::room::EuphRoom;
use super::widgets::background::Background;
use super::widgets::border::Border;
use super::widgets::editor::EditorState;
use super::widgets::float::Float;
use super::widgets::join::{HJoin, Segment, VJoin};
use super::widgets::layer::Layer;
use super::widgets::list::{List, ListState};
use super::widgets::padding::Padding;
use super::widgets::text::Text;
use super::widgets::BoxedWidget;
use super::UiEvent;

enum State {
    ShowList,
    ShowRoom(String),
    Connect(EditorState),
}

pub struct Rooms {
    vault: Vault,
    ui_event_tx: mpsc::UnboundedSender<UiEvent>,

    state: State,

    list: ListState<String>,
    euph_rooms: HashMap<String, EuphRoom>,
}

impl Rooms {
    pub fn new(vault: Vault, ui_event_tx: mpsc::UnboundedSender<UiEvent>) -> Self {
        Self {
            vault,
            ui_event_tx,
            state: State::ShowList,
            list: ListState::new(),
            euph_rooms: HashMap::new(),
        }
    }

    /// Remove rooms that are not running any more and can't be found in the db.
    ///
    /// These kinds of rooms are either
    /// - failed connection attempts, or
    /// - rooms that were deleted from the db.
    async fn stabilize_rooms(&mut self) {
        let rooms_set = self
            .vault
            .euph_rooms()
            .await
            .into_iter()
            .collect::<HashSet<_>>();
        self.euph_rooms
            .retain(|n, r| !r.stopped() || rooms_set.contains(n));

        for room in self.euph_rooms.values_mut() {
            room.retain();
        }
    }

    async fn room_names(&self) -> Vec<String> {
        let mut rooms = self.vault.euph_rooms().await;
        for room in self.euph_rooms.keys() {
            rooms.push(room.clone());
        }
        rooms.sort_unstable();
        rooms.dedup();
        rooms
    }

    fn get_or_insert_room(&mut self, name: String) -> &mut EuphRoom {
        self.euph_rooms
            .entry(name.clone())
            .or_insert_with(|| EuphRoom::new(self.vault.euph(name), self.ui_event_tx.clone()))
    }

    pub async fn widget(&mut self) -> BoxedWidget {
        match &self.state {
            State::ShowRoom(_) => {}
            _ => self.stabilize_rooms().await,
        }

        match &self.state {
            State::ShowList => self.rooms_widget().await,
            State::ShowRoom(name) => self.get_or_insert_room(name.clone()).widget().await,
            State::Connect(ed) => {
                let room_style = ContentStyle::default().bold().blue();
                Layer::new(vec![
                    self.rooms_widget().await,
                    Float::new(Border::new(Background::new(
                        Padding::new(VJoin::new(vec![
                            Segment::new(Text::new("Connect to ")),
                            Segment::new(HJoin::new(vec![
                                Segment::new(Text::new(("&", room_style))),
                                Segment::new(ed.widget().highlight(|s| Styled::new(s, room_style))),
                            ])),
                        ]))
                        .left(1),
                    )))
                    .horizontal(0.5)
                    .vertical(0.5)
                    .into(),
                ])
                .into()
            }
        }
    }

    fn format_pbln(joined: &Joined) -> String {
        let mut p = 0_usize;
        let mut b = 0_usize;
        let mut l = 0_usize;
        let mut n = 0_usize;
        for sess in iter::once(&joined.session).chain(joined.listing.values()) {
            match sess.id.session_type() {
                Some(SessionType::Bot) if sess.name.is_empty() => n += 1,
                Some(SessionType::Bot) => b += 1,
                _ if sess.name.is_empty() => l += 1,
                _ => p += 1,
            }
        }

        // There must always be either one p, b, l or n since we're including
        // ourselves.
        let mut result = vec![];
        if p > 0 {
            result.push(format!("{p}p"));
        }
        if b > 0 {
            result.push(format!("{b}b"));
        }
        if l > 0 {
            result.push(format!("{l}l"));
        }
        if n > 0 {
            result.push(format!("{n}n"));
        }
        result.join(" ")
    }

    fn format_status(status: &Option<Status>) -> String {
        match status {
            None => " (connecting)".to_string(),
            Some(Status::Joining(j)) if j.bounce.is_some() => " (auth required)".to_string(),
            Some(Status::Joining(_)) => " (joining)".to_string(),
            Some(Status::Joined(j)) => format!(" ({})", Self::format_pbln(j)),
        }
    }

    async fn render_rows(&self, list: &mut List<String>, rooms: Vec<String>) {
        let heading_style = ContentStyle::default().bold();
        let heading = Styled::new("Rooms", heading_style).then_plain(format!(" ({})", rooms.len()));
        list.add_unsel(Text::new(heading));

        for room in rooms {
            let bg_style = ContentStyle::default();
            let bg_sel_style = ContentStyle::default().black().on_white();
            let room_style = ContentStyle::default().bold().blue();
            let room_sel_style = ContentStyle::default().bold().black().on_white();

            let mut normal = Styled::new(format!("&{room}"), room_style);
            let mut selected = Styled::new(format!("&{room}"), room_sel_style);
            if let Some(room) = self.euph_rooms.get(&room) {
                if let Some(status) = room.status().await {
                    let status = Self::format_status(&status);
                    normal = normal.then(status.clone(), bg_style);
                    selected = selected.then(status, bg_sel_style);
                }
            };

            list.add_sel(
                room,
                Text::new(normal),
                Background::new(Text::new(selected)).style(bg_sel_style),
            );
        }
    }

    async fn rooms_widget(&self) -> BoxedWidget {
        let rooms = self.room_names().await;
        let mut list = self.list.widget().focus(true);
        self.render_rows(&mut list, rooms).await;
        list.into()
    }

    pub async fn list_key_bindings(&self, bindings: &mut KeyBindingsList) {
        match &self.state {
            State::ShowList => {
                bindings.heading("Rooms");
                bindings.binding("j/k, ↓/↑", "move cursor up/down");
                bindings.binding("g, home", "move cursor to top");
                bindings.binding("G, end", "move cursor to bottom");
                bindings.binding("ctrl+y/e", "scroll up/down");
                bindings.empty();
                bindings.binding("enter", "enter selected room");
                bindings.binding("c", "connect to selected room");
                bindings.binding("C", "connect to new room");
                bindings.binding("d", "disconnect from selected room");
                bindings.binding("D", "delete room");
            }
            State::ShowRoom(name) => {
                // Key bindings for leaving the room are a part of the room's
                // list_key_bindings function since they may be shadowed by the
                // nick selector or message editor.
                if let Some(room) = self.euph_rooms.get(name) {
                    room.list_key_bindings(bindings).await;
                } else {
                    // There should always be a room here already but I don't
                    // really want to panic in case it is not. If I show a
                    // message like this, it'll hopefully be reported if
                    // somebody ever encounters it.
                    bindings.binding_ctd("oops, this text should never be visible")
                }
            }
            State::Connect(_) => {
                bindings.heading("Rooms");
                bindings.binding("esc", "abort");
                bindings.binding("enter", "connect to room");
                bindings.binding("←/→", "move cursor left/right");
                bindings.binding("backspace", "delete before cursor");
                bindings.binding("delete", "delete after cursor");
            }
        }
    }

    pub async fn handle_key_event(
        &mut self,
        terminal: &mut Terminal,
        crossterm_lock: &Arc<FairMutex<()>>,
        event: KeyEvent,
    ) -> bool {
        match &self.state {
            State::ShowList => match event {
                key!('k') | key!(Up) => self.list.move_cursor_up(),
                key!('j') | key!(Down) => self.list.move_cursor_down(),
                key!('g') | key!(Home) => self.list.move_cursor_to_top(),
                key!('G') | key!(End) => self.list.move_cursor_to_bottom(),
                key!(Ctrl + 'y') => self.list.scroll_up(1),
                key!(Ctrl + 'e') => self.list.scroll_down(1),

                key!(Enter) => {
                    if let Some(name) = self.list.cursor() {
                        self.state = State::ShowRoom(name);
                    }
                }
                key!('c') => {
                    if let Some(name) = self.list.cursor() {
                        self.get_or_insert_room(name).connect();
                    }
                }
                key!('C') => self.state = State::Connect(EditorState::new()),
                key!('d') => {
                    if let Some(name) = self.list.cursor() {
                        self.get_or_insert_room(name).disconnect();
                    }
                }
                key!('D') => {
                    // TODO Check whether user wanted this via popup
                    if let Some(name) = self.list.cursor() {
                        self.euph_rooms.remove(&name);
                        self.vault.euph(name.clone()).delete();
                    }
                }
                _ => return false,
            },
            State::ShowRoom(name) => {
                if self
                    .get_or_insert_room(name.clone())
                    .handle_key_event(terminal, crossterm_lock, event)
                    .await
                {
                    return true;
                }

                if let key!(Esc) = event {
                    self.state = State::ShowList;
                    return true;
                }

                return false;
            }
            State::Connect(ed) => match event {
                key!(Esc) => self.state = State::ShowList,
                key!(Enter) => {
                    let name = ed.text();
                    if !name.is_empty() {
                        self.get_or_insert_room(name.clone()).connect();
                        self.state = State::ShowRoom(name);
                    }
                }
                key!(Char ch) if ch.is_ascii_alphanumeric() || ch == '_' => ed.insert_char(ch),
                key!(Left) => ed.move_cursor_left(),
                key!(Right) => ed.move_cursor_right(),
                key!(Backspace) => ed.backspace(),
                key!(Delete) => ed.delete(),
                _ => return false,
            },
        }

        true
    }
}
