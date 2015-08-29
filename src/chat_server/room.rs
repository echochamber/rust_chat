use std::collections::HashMap;
use mio;
use super::user::Username;

pub type Roomname = String;

pub struct ChatRoom {
	pub name: Roomname,
	pub members: HashMap<mio::Token, Username>
}