extern crate mio;
extern crate bytes;

mod chat_server;

pub fn main() {
    let mut chat_app = chat_server::ChatApp::new();
    chat_app.start("0.0.0.0:6567".parse().unwrap());
}