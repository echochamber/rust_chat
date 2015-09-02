

pub enum ChatCommand {
	ListRooms,
	// ListRoomMembers(String), Todo
	ChangeRoom(String),
	Quit
}

impl ChatCommand {
	pub fn new(command: &String) -> Option<ChatCommand> {
		// Remove the leading '/'

		let mut split = command.split_whitespace();

		match split.next() {
			Some("/rooms") => {
				return Some(ChatCommand::ListRooms)
			},
			Some("/quit") => {
				return Some(ChatCommand::Quit)
			},
			Some("/join") => {
				match split.next() {
					Some(room_name) => {
						return Some(ChatCommand::ChangeRoom(room_name.to_string()))
					},
					// Missing the room name to change to
					None => {
						return None;
					}
				}
			},
			Some(_) => {
				// Invalid command name
				return None;
			},
			None => {
				// No command name was supplied
				return None;
			}
		}
	}
}

pub fn is_command(message: &String) -> bool {
	return message.starts_with('/');
}