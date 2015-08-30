use mio;
use mio::{Token, EventLoop, EventSet, PollOpt};
use mio::tcp::*;
use mio::util::Slab;
use bytes::{Take, ByteBuf};
use std::mem;
use std::net::SocketAddr;
use std::io::Cursor;
use std::io;
use std::collections::HashMap;
use std::rc::Rc;

use super::app::ChatApp;
use super::connection::ChatConnection;
use super::user::{ChatUser, Username};
use super::room::{ChatRoom, Roomname};

/// The token for the tcp listener socket.
/// kqueue has some wierd behaviors when the server is Token(0) so we'll use token 1.
pub const SERVER_TOKEN: Token = Token(1);

/// Represents the server's connection for the chat app
pub struct ChatServer {
    /// The tcp connection the server listens on
    server: TcpListener,

    /// All the connections to the chat server, indexed by their token.
    connections: Slab<ChatConnection>,

    app: ChatApp
}

impl ChatServer {
    // Initialize a new `ChatServer` server from the given TCP listener socket
    pub fn new(server: TcpListener) -> ChatServer {

        ChatServer {
            server: server,
            connections: Slab::new_starting_at(Token(SERVER_TOKEN.0 + 1), 1024),
            app: ChatApp::new()
        }
    }

    /// Function that is called when the chat server recieves a call to ready and the event set contains readable
    /// Handles all logic related to reading from any connection besides the server connection
    fn read(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) -> io::Result<()> {
        let mut ret = Ok(());
        if let Some(message) = self.connections[token].read(event_loop) {

            self.write_message_from_token(event_loop, token, message);

            // Remove all bad connections or connections that have been closed
            
            if self.connections[token].is_closed() {
                self.connections.remove(token);
            }
        }

        return ret;
    }

    fn write(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) -> io::Result<()> {
        // Should never recieve write events for the server connection
        assert!(SERVER_TOKEN != token, "Received writable event for the server's token");

        super::log_something(format!("Write event for {:?}", token));

        match self.get_connection(token).write() {
            Ok(_) => {
                self.get_connection(token).reregister(event_loop);
                return Ok(());
            },
            Err(e) => {
                super::log_something(format!("Write event failed for {:?}, {:?}", token, e));
                self.reset_connection(event_loop, token);
                return Err(e);
            }
        }
    }

    fn write_message_from_token(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token, message: Vec<u8>) {
        let mut bad_conn_tokens = Vec::new();


        // Handle the case where the connection we read the message from has already registered a user
        if let Some(username) = self.app.get_username(token) {

            // Todo handle commands if the message starts with a /
            let mut mes_with_sender = username.into_bytes();
            mes_with_sender.extend(": ".as_bytes());
            mes_with_sender.extend(message);
            
            let mes_rc = Rc::new(mes_with_sender);

            {
                let tokens = self.app.get_message_recipients(token);
                for token in tokens {
                    let conn = &mut self.get_connection(token);
                    conn.send_message(mes_rc.clone());
                    conn.reregister(event_loop)
                        .unwrap_or_else(|e| {
                            bad_conn_tokens.push(token);
                    });
                }
            }

            for bad_token in bad_conn_tokens {
                self.reset_connection(event_loop, bad_token);
            }

            return;
        };

        // Handle the case where the connection we read the message from has not registered a user yet

        // TODO: Create a username with the message contents if we can.
        // If we can't then queue a message to be sent to the connection telling them to pick a different username.
    }

    /// If the server connection needs to be reset, then that means the application should be shut down.
    fn reset_connection(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) {
        if SERVER_TOKEN == token {
            event_loop.shutdown();
        } else {
            self.connections.remove(token);
        }
    }

    fn get_connection<'a>(&'a mut self, token: Token) -> &'a mut ChatConnection {
        &mut self.connections[token]
    }

    /// Function that is called when the chat server recieves a call to ready with its own token and a readable EventSet
    /// Accept a new connection
    fn accept(&mut self, event_loop: &mut EventLoop<ChatServer>) -> Result<(), String> {

        // Log an error if there is no socket
        let sock = match self.server.accept() {
            Ok(Some(socket)) => { socket },
            Ok(None) => {
                return Err("Failed to accept new socket".to_string());
            },
            Err(e) => {
                return Err(format!("Failed to accept new socket, {:?}", e));
            }
        };

        // If there was a socket, then register a new connection with it.
        match self.connections.insert_with(|token| {ChatConnection::new(sock, token)}) {
            // If we successfully insert, then register our connection.
            Some(token) => {
                self.get_connection(token).send_message(Rc::new("Server: Select a username:\n".into()));
                match self.get_connection(token).register(event_loop) {
                    Ok(_) => {},
                    Err(e) => {
                        self.connections.remove(token);
                        return Err(format!("Failed to register {:?} connection with event loop, {:?}", token, e));
                    }
                }
            },
            None => {
                return Err("Failed to insert connection into slab".to_string());
            }
        };
       
        return Ok(())
    }

    /// Since the socket is registered
    fn reregister(&mut self, event_loop: &mut EventLoop<ChatServer>) {
        event_loop.reregister(
            &self.server,
            SERVER_TOKEN,
            EventSet::readable(),
            PollOpt::edge() | PollOpt::oneshot()
        ).unwrap_or_else(|e| {
            super::log_something(format!("Failed to reregister server {:?}, {:?}", SERVER_TOKEN, e));
            self.reset_connection(event_loop, SERVER_TOKEN);
        })
    }
}

impl mio::Handler for ChatServer {
    type Timeout = (); // TODO
    type Message = (); // Since the chat server is only single threaded, no need to worry about this.
    // If it was multitreaded, all instances of Rc would need to be changed to Arc instead.

    // Called by the EventLoop whenever a socket is ready to be acted on.
    // Is passed the token for that socket and the current EventSet that socket is ready for.
    fn ready(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token, events: mio::EventSet) {
        super::log_something(format!("socket is ready; token={:?}; events={:?}", token, events));

        if events.is_error() {
            super::log_something(format!("Error event for {:?}", token));
            self.reset_connection(event_loop, token);
            return;
        }

        if events.is_hup() {
            super::log_something(format!("Hup event for {:?}", token));
            self.reset_connection(event_loop, token);
            return;
        }

        if events.is_writable() {
            super::log_something(format!("Write event for {:?}", token));
            assert!(SERVER_TOKEN != token, "Received writable event for Server");

            self.get_connection(token).write()
                .and_then(|_| self.get_connection(token).reregister(event_loop))
                .unwrap_or_else(|e| {
                    super::log_something(format!("Write event failed for {:?}, {:?}", token, e));
                    self.reset_connection(event_loop, token);
                });
        }


        if events.is_readable() {
            super::log_something(format!("Read event for {:?}", token));
            if SERVER_TOKEN == token {
                self.accept(event_loop);
                self.reregister(event_loop);
            } else {

                self.read(event_loop, token)
                    .and_then(|_| self.get_connection(token).reregister(event_loop))
                    .unwrap_or_else(|e| {
                        super::log_something(format!("Read event failed for {:?}: {:?}", token, e));
                        self.reset_connection(event_loop, token);
                    });
            }
        }
    }
}