use mio::Token;
use super::room::Roomname;

pub type Username = String;

pub struct ChatUser {
    pub id: Token,
    pub user_name: Username,
    pub location: Roomname
}
