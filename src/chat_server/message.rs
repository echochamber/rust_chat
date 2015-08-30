pub fn is_command(message: &String) -> bool {
	return message.starts_with('/');
}