use mio;
use mio::{Token, EventLoop, EventSet, PollOpt};
use mio::tcp::*;
use mio::util::Slab;
use time;

use std::io::ErrorKind;
use std::rc::Rc;

use super::app::ChatApp;
use super::connection::ChatConnection;
use super::command::{is_command, ChatCommand};

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
    fn read(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) {

        // If we get Some back, then the message has been fully recieved and we can handle it accordingly
        match self.connections[token].read()
        {
            Ok(Some(message)) => {
                self.handle_message_read_from_client(event_loop, token, message);
            },
            Ok(None) => {
                // Nothing was read from the client, or a newline has not be encountered yet
                // Either way, just keep listening.
            }
            Err(e) => {
                self.handle_error_when_reading_from_client(token, e);
            }
        }

        if self.connections[token].is_closed() {
            self.reset_connection(event_loop, token);
        } else {
            self.reregister(event_loop, token);
        }
    }

    /// Function that is called when the chat server recieves a call to ready and the event set contains writable
    /// Handles all logic related to writing to any client connections
    fn write(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) {
        super::log_something(format!("Write event for {:?}", token));
        assert!(SERVER_TOKEN != token, "Received writable event for Server");

        self.get_connection(token).write();

        if self.connections[token].is_closed() {
            self.reset_connection(event_loop, token);
        } else {
            self.reregister(event_loop, token);
        }
    }

    fn handle_error_when_reading_from_client(&mut self, token: Token, error: ::std::io::Error) {
        // TODO, maybe need different behavior for different variants?
        match error.kind() {
            ErrorKind::InvalidInput => {
                super::log_something("Data read from connection was not valid utf8");
                self.connections[token].send_message(Rc::new("Server: Invalid utf8, message was discarded.\n".to_string().into_bytes()));
            },
            _ => {
            }
        };
    }

    fn handle_message_read_from_client(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token, message: String) {
        if is_command(&message) {
            self.handle_command_message(event_loop, token, &message);
            return;
        }

        if let Some(username) = self.app.get_username(token) {
            self.handle_message_from_authorized_user(event_loop, token, username, message);
            return;
        }

        self.handle_message_from_unauthorized_user(token, message);
    }

    fn handle_message_from_unauthorized_user(&mut self, token: Token, message: String) {
        // We could validate that this message has no whitepspace, but for now just take the first piece of the message
        // split by whitespace and use that as the clients username.
        match message.split(char::is_whitespace).nth(0) {
            Some(name) => {
                match self.app.register_user(token, name.to_string()) {
                    Ok(_) => {
                        let conn = self.get_connection(token);
                        conn.send_message(Rc::new("Server: you have been successfully authorized\n".to_string().into_bytes()));
                    },
                    Err(e) => {
                        super::log_something(format!("{}", e));
                        self.connections[token].send_message(Rc::new("Server: That username is taken, please try another\n".to_string().into_bytes()))
                    }
                }
            },
            None => {
                // Do nothing, the client sent either just a newline or newline + whitespace
            }
        }
    }

    /// The user is sending a message to their current room. Create a shared pointer to the message and queue it up to be send to every
    /// client in that same room the next time a write event for that client is recieved.
    fn handle_message_from_authorized_user(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token, username: String, message: String) {
        let mut bad_conn_tokens: Vec<Token> = Vec::new();
        let timestamp = time::strftime("%Y:%m:%d %H:%M:%S", &time::now()).unwrap().into_bytes();
        
        // TODO: This could definitely be done more optimally but it works for now.
        let mut mes_with_sender: Vec<u8> = timestamp;
        mes_with_sender.extend(" - ".as_bytes());
        mes_with_sender.extend(username.into_bytes());
        mes_with_sender.extend(": ".as_bytes());
        mes_with_sender.extend(message.as_bytes());
        
        let mes_rc = Rc::new(mes_with_sender);

        // Enter a new scope so the borrow ends before we reset connections for bad tokens
        {
            let tokens = self.app.get_message_recipients(token);
            for &recipient_token in tokens.iter() {
                let conn = self.get_connection(recipient_token);
                conn.send_message(mes_rc.clone());
                if conn.reregister(event_loop).is_err() {
                    bad_conn_tokens.push(recipient_token);
                }
            }
        }

        for bad_token in bad_conn_tokens {
            self.reset_connection(event_loop, bad_token);
        }
    }

    /// Handle messages starting with a /. Currently, if the command doesn't match one of our existing commands we don't do anything
    fn handle_command_message(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token, message: &String) {
        match ChatCommand::new(message) {
            Some(ChatCommand::ListRooms) => {
                let mut list = String::new();
                for room_name in self.app.get_room_list() {
                    list.push_str(room_name.as_str());
                    list.push('\n');
                }
                let conn = self.get_connection(token);
                conn.send_message(Rc::new(list.clone().into_bytes()));
                conn.reregister(event_loop);
            },
            Some(ChatCommand::Quit) => {
                let conn = self.get_connection(token);
                conn.quit();
            },
            Some(ChatCommand::ChangeRoom(room_name)) => {
                self.app.move_rooms(token, &room_name);

                let conn = self.get_connection(token);
                conn.send_message(Rc::new(format!("Moved to room {}\n", room_name).to_string().into_bytes()));
                conn.reregister(event_loop);
            }
            None => {
                let conn = self.get_connection(token);
                conn.send_message(Rc::new("Not a valid command\n".to_string().into_bytes()));
                conn.reregister(event_loop);
            }
        }

        
        super::log_something(format!("Command read {}", message));
    }

    /// If the server connection needs to be reset, then that means the application should be shut down.
    fn reset_connection(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) {
        if SERVER_TOKEN == token {
            event_loop.shutdown();
        } else {
            self.connections[token].deregister(event_loop);
            self.connections.remove(token);
            self.app.remove_user(token);
        }
    }

    /// Reregister a connection with the event loop
    fn reregister(&mut self, event_loop: &mut EventLoop<ChatServer>, token: Token) {
        if token == SERVER_TOKEN {
            event_loop.reregister(
                &self.server,
                SERVER_TOKEN,
                EventSet::readable(),
                PollOpt::edge() | PollOpt::oneshot()
            ).unwrap_or_else(|e| {
                super::log_something(format!("Failed to reregister server {:?}, {:?}", SERVER_TOKEN, e));
                self.reset_connection(event_loop, SERVER_TOKEN);
            });
        } else {
            // Todo, figure out the behavior when we we fail to reregister a client connection
            self.connections[token].reregister(event_loop);
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

                match self.get_connection(token).register(event_loop) {
                    Ok(_) => {},
                    Err(e) => {
                        self.connections.remove(token);
                        return Err(format!("Failed to register {:?} connection with event loop, {:?}", token, e));
                    }
                }

                match self.app.get_username(token) {
                    Some(username) => {
                        self.get_connection(token).send_message(Rc::new(format!("Server: Welcome back {}:\n", username).into()));
                    },
                    None => {
                        self.get_connection(token).send_message(Rc::new("Server: Select a username:\n".into()));
                    }
                }
            },
            None => {
                return Err("Failed to insert connection into slab".to_string());
            }
        };
       
        return Ok(())
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
            self.write(event_loop, token);
        }


        if events.is_readable() {
            super::log_something(format!("Read event for {:?}", token));
            if SERVER_TOKEN == token {
                self.accept(event_loop);
                self.reregister(event_loop, SERVER_TOKEN);
            } else {

                self.read(event_loop, token);
            }
        }
    }
}