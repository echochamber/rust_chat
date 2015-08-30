use mio;
use super::user::Username;

pub type Roomname = String;

pub struct ChatRoom {
	pub name: Roomname,
	pub members: Vec<mio::Token>
}

impl ChatRoom {
	pub fn new(name: Roomname) -> ChatRoom {
		ChatRoom {
			name: name,
			members: Vec::new()
		}
	}
}