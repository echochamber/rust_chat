use mio;
use super::connection::ChatConnection;
use super::room::Roomname;

pub type Username = String;

pub struct ChatUser {
    pub id: mio::Token,
    pub user_name: Username,
    pub location: Roomname
}
