use std::str::FromStr;
use std::mem;
use std::net::SocketAddr;
use std::io::Cursor;
use std::io;
use std::rc::Rc;

use mio;
use mio::{Token, EventLoop, EventSet, TryRead, TryWrite, PollOpt};
use mio::tcp::{TcpStream};
use mio::util::Slab;
use bytes::{Buf, Take, ByteBuf};

use super::server::ChatServer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatConnectionState {
    Open,
    Closed
}

/// Represents a single connection to the chat server.
pub struct ChatConnection {
    /// The TCP socket
    socket: TcpStream,

    /// The token that was used to register the socket with the `EventLoop`
    token: mio::Token,
    
    // Events this connection is interested in listening on
    interest: EventSet,

    /// Buffer of bytes read from this connection. 
    ///
    /// Is cleared every time a newline character is received and the
    /// contents will be queued up to be written to all the other connections
    read_buf: Vec<u8>,

    /// A queue of reference counted references to bytebuffers
    /// 
    /// Each bytebuffer represents a message queued to write to this connection
    /// the next time it becomes ready to be written to
    send_queue: Vec<Rc<Vec<u8>>>,

    /// Is this connection open/closed
    state: ChatConnectionState
}

impl ChatConnection {
    pub fn new(socket: TcpStream, token: mio::Token) -> ChatConnection {
        ChatConnection {
            socket: socket,
            token: token,
            interest: EventSet::readable(),
            // Should be done with_capacity for a reasonable message size
            read_buf: Vec::new(),
            send_queue: Vec::new(),
            state: ChatConnectionState::Open
        }
    }

    /// Returns Some if the read_buf is ready to be written to the other connections
    /// Otherwise, return none and continue reading into the read_buf until ready 
    ///
    /// Clears the read buff when it returns Some
    ///
    /// Returns an RC (Thread-local reference-counted box) so that we can just copy a reference to each
    /// connections send_queue, and once they have all been written to, the reference count should drop
    /// to 0 and they the vec should automatically be freed.
    pub fn read(&mut self, event_loop: &mut mio::EventLoop<ChatServer>) -> Option<String> {
        match self.socket.try_read_buf(&mut self.read_buf) {
            // 0 Bytes were read
            Ok(Some(0)) => {
                // 0 bytes were read and the buffer is empty, the connection has been closed by the client
                if self.read_buf.len() > 0 {
                    self.state = ChatConnectionState::Closed
                }
            }

            // n bytes were read
            Ok(Some(n)) => {
                super::log_something(format!("read {} bytes", n));

                // The conditions have been met so that the input read from this connection
                // is now ready to be written to the other clients
                if let Some(limit) = self.is_ready_to_write() {
                    self.reregister(event_loop);

                    // Clear the current read buffer, but keep a handle to it since we will be returning it
                    // so that the server can add it to the other connection's send_queue's
                    let mut read_buf = mem::replace(&mut self.read_buf, Vec::new());

                    // Alternatively we could return bytes::Take(Cursor::new(read_buf), limit)
                    // which is just an iterator that returns eof once its iterated over "limit" elements.
                    self.read_buf.truncate(limit);
                    match String::from_utf8(read_buf) {
                        Ok(message) => {
                            return Some(message);
                        },
                        Err(e) => {
                            super::log_something("Data read from connection was not valid utf8");
                            return None;
                        }
                    }
                };
            }
            // The socket's a liar! It wasn't actually ready for us to read from. 
            // Nothing we need to do here. Just keep listening same as before.
            Ok(None) => {}
            Err(e) => {
                // Probably shouldn't panic here since this will abort the entire application
                // due to an error that only relates to a single connection.
                panic!("Error reading from connection token={:?}; err={:?}", self.token, e);
            }
        }

        // As long as the connection is still open, we want to register it as a listener for read and write events again.
        if self.state == ChatConnectionState::Open {
            self.reregister(event_loop);
        }

        return None;
    }

    /// Writes to the connection, using the next entry from the send_queue.
    /// 
    /// Only the next entry in the send_queue will be sent per call. It may be better to just send 
    /// them all at once, separated by newlines.
    pub fn write(&mut self) -> io::Result<()> {
        let res = match self.send_queue.pop() {
            Some(buf) => {
                match self.socket.try_write_buf(&mut Cursor::new(buf.to_vec())) {
                    Ok(None) => {
                        super::log_something(format!("client flushing buf; WouldBlock"));

                        // Put message back into the queue so we can try again
                        self.send_queue.insert(0, buf);
                        Ok(())
                    },
                    Ok(Some(n)) => {
                        super::log_something(format!("CONN : we wrote {} bytes", n));
                        Ok(())
                    },
                    Err(e) => {
                        super::log_something(format!("Failed to send buffer for {:?}, error: {}", self.token, e));
                        Err(e)
                    }
                }
            }
            None => {
                Err(io::Error::new(io::ErrorKind::Other, "Could not pop send queue"))
            }
        };

        // If that was the last message in this connections send queue, 
        // then we don't need to listen for writes until another message gets added.
        if self.send_queue.is_empty() {
            self.interest.remove(EventSet::writable());
        }

        return res;
    }

    pub fn is_closed(&self) -> bool {
        return self.state == ChatConnectionState::Closed;
    }

    /// Queues a message up to be written to this connection the next time it recieves a call to write
    /// If this connection was not subscribed to write events before, it is now.
    pub fn send_message(&mut self, message: Rc<Vec<u8>>) {
        self.send_queue.push(message);
        self.interest.insert(EventSet::writable());
    }

    // When we 
    pub fn register(&self, event_loop: &mut mio::EventLoop<ChatServer>) -> io::Result<()> {
        event_loop.register_opt(
            &self.socket,
            self.token,
            self.interest,
            mio::PollOpt::edge() | mio::PollOpt::oneshot()
        )
    }

    pub fn reregister(&self, event_loop: &mut mio::EventLoop<ChatServer>) -> io::Result<()> {
        event_loop.reregister(
            &self.socket,
            self.token,
            self.interest,
            PollOpt::edge() | PollOpt::oneshot()
        )
    }

    pub fn deregister(&self, event_loop: &mut mio::EventLoop<ChatServer>) -> io::Result<()> {
        event_loop.deregister(&self.socket)
    }

    pub fn token(&self) -> Token {
        self.token
    }

    /// Does this correctly handle mutlibyte utf8 characters currently? 
    ///
    /// If the connection is ready to write to the other connections, return Some with
    /// the number of bytes to take from the read buffer to write to the other connections
    /// Otherwise return None
    fn is_ready_to_write(&self) -> Option<usize> {
        return match self.read_buf.iter().position(|b| *b == b'\n') {
            Some(pos) => {
                Some(pos + 1)
            },
            None => {
                None
            }
        };
    }
}
