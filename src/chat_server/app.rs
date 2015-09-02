use std::collections::HashMap;
use mio::Token;

use super::user::{ChatUser, Username};
use super::room::{ChatRoom, Roomname};

pub struct ChatApp {
	/// Hashmap of connections with a registered username
    users: HashMap<Token, ChatUser>,

    /// Hashmap of rooms currently available
    rooms: HashMap<Roomname, ChatRoom>,

    /// Hashmap of usernames => tokens for quick lookup and to prevent different connections
    /// from claiming the same username
    user_name_lookup: HashMap<Username, Token>
}

impl<'a> ChatApp {

	pub fn new() -> ChatApp {
		let mut app = ChatApp {
			users: HashMap::new(),
			rooms: HashMap::new(),
			user_name_lookup: HashMap::new()
		};

		app.rooms.insert("default".to_string(), ChatRoom::new("default".to_string()));

		app
	}

	/// If the given token were to send a message, return the list tokens for connections that should recieve that message.
	pub fn get_message_recipients(&self, sender: Token) -> Vec<Token> {
		let room_name = &self.users.get(&sender).unwrap().location;
		return self.rooms.get(room_name).unwrap().members.iter().cloned().collect();
	}

	pub fn get_room_list(&self) -> Vec<Roomname> {
		self.rooms.keys().cloned().collect()
	}

	pub fn get_username(&self, token: Token) -> Option<Username> {
		match self.users.get(&token) {
			Some(user) => {
				return Some(user.user_name.clone());
			},
			None => {
				return None;
			}
		};
	}

	pub fn move_rooms(&mut self, token: Token, dest: &Roomname) {

		// Create the room if it doesn't exist yet
		if !self.rooms.contains_key(dest) {
			self.rooms.insert(dest.clone(), ChatRoom::new(dest.clone()));
		}

		let user = self.users.get_mut(&token).unwrap();

		self.rooms.get_mut(&user.location).unwrap().members.remove(&token);

		user.location = dest.clone();
		self.rooms.get_mut(dest).unwrap().members.insert(token);
	}

	/// Returns true if the user was registered, false otherwise.
	pub fn register_user(&mut self, token: Token, user_name: Username) -> Result<(), String> {
		if self.users.contains_key(&token) {
			return Err("A user is already registered for that token".into());
		}

		// This is not correctly detecting collisions yet
		if self.user_name_lookup.get(&user_name).is_some() {
			println!("Collision");
			return Err("A user with that name is already registered".into());
		}

		let user = ChatUser {
			id: token,
			user_name: user_name.clone(),
			location: "default".into()
		};

		self.rooms.get_mut("default".into()).unwrap().members.insert(token);
		self.users.insert(token, user);
		self.user_name_lookup.insert(user_name, token);

		return Ok(());
	}

	pub fn remove_user(&mut self, token: Token) {
		match self.users.remove(&token) {
			Some(user) => {
				self.rooms.get_mut(&user.location).unwrap().members.remove(&token);
				self.user_name_lookup.remove(&user.user_name);
			},
			None => {}
		}
	}
}