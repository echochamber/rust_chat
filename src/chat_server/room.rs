use mio;
use super::user::Username;
use std::collections::HashSet;

pub type Roomname = String;

pub struct ChatRoom {
	pub name: Roomname,
	pub members: HashSet<mio::Token>
}

impl ChatRoom {
	pub fn new(name: Roomname) -> ChatRoom {
		ChatRoom {
			name: name,
			members: HashSet::new()
		}
	}
}