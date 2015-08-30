mod server;
mod connection;
mod user;
mod room;
mod app;

use std::net::SocketAddr;
use mio;
use mio::{Token, EventLoop};
use mio::tcp::TcpListener;
use self::connection::*;
use self::server::*;

// Easy logging for now
pub fn log_something<T: ::std::fmt::Debug>(logged_thing: T) {
    println!("{:?}", logged_thing)
} 

pub fn run_server(address: SocketAddr) {
	// Create a new non-blocking socket bound to the given address. All sockets
    // created by mio are set to non-blocking mode.
    let server = TcpListener::bind(&address).unwrap();

    // Create a new `EventLoop`. 
    let mut event_loop = EventLoop::new().unwrap();

    // Register the server socket with the event loop.
    event_loop.register(&server, server::SERVER_TOKEN).unwrap();

    // Create a new `ChatServer` instance that will track the state of the server.
    let mut pong = ChatServer::new(server);

    // Run the `ChatServer` server
    println!("running pingpong server; port=6567");
    event_loop.run(&mut pong).unwrap();
}
