use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::macros::ok_or_return;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomsSortOrder {
    #[default]
    Alphabet,
    Importance,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EuphRoom {
    // TODO Mark favourite rooms via printable ascii characters
    #[serde(default)]
    pub autojoin: bool,
    pub username: Option<String>,
    #[serde(default)]
    pub force_username: bool,
    pub password: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Euph {
    pub rooms: HashMap<String, EuphRoom>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub data_dir: Option<PathBuf>,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub rooms_sort_order: RoomsSortOrder,
    // TODO Invoke external notification command?
    pub euph: Euph,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let content = ok_or_return!(fs::read_to_string(path), Self::default());
        match toml::from_str(&content) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Error loading config file: {err}");
                Self::default()
            }
        }
    }

    pub fn euph_room(&self, name: &str) -> EuphRoom {
        self.euph.rooms.get(name).cloned().unwrap_or_default()
    }
}
