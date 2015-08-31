use mio::Token;
use std::collections::HashSet;

pub type Roomname = String;

pub struct ChatRoom {
	pub name: Roomname,
	pub members: HashSet<Token>
}

impl ChatRoom {
	pub fn new(name: Roomname) -> ChatRoom {
		ChatRoom {
			name: name,
			members: HashSet::new()
		}
	}
}