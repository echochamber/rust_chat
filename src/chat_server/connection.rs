use std::mem;
use std::collections::vec_deque::VecDeque;
use std::io;
use std::io::Cursor;
use std::io::ErrorKind;
use std::rc::Rc;

use mio;
use mio::{Token, EventLoop, EventSet, TryRead, TryWrite, PollOpt};
use mio::tcp::{TcpStream};

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
    pub interest: EventSet,

    /// Buffer of bytes read from this connection. 
    ///
    /// Is cleared every time a newline character is received and the
    /// contents will be queued up to be written to all the other connections
    read_buf: Vec<u8>,

    /// A queue of reference counted references to bytebuffers
    /// 
    /// Each bytebuffer represents a message queued to write to this connection
    /// the next time it becomes ready to be written to
    send_queue: VecDeque<Rc<Vec<u8>>>,

    /// Is this connection open/closed
    state: ChatConnectionState,

    /// Number of failed read attempts on the socket, currently abort after 3
    failed_read_attempts: u32,

    /// Number of failed write attempts on the socket, currently abort after 3
    failed_write_attempts: u32
}

impl ChatConnection {
    pub fn new(socket: TcpStream, token: mio::Token) -> ChatConnection {
        ChatConnection {
            socket: socket,
            token: token,
            interest: EventSet::readable() | EventSet::writable(),
            // Should be done with_capacity for a reasonable message size
            read_buf: Vec::new(),
            send_queue: VecDeque::new(),
            state: ChatConnectionState::Open,
            failed_read_attempts: 0,
            failed_write_attempts: 0
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
    pub fn read(&mut self) -> io::Result<Option<String>> {
        match self.socket.try_read_buf(&mut self.read_buf) {
            // 0 Bytes were read
            Ok(Some(0)) => {
                self.state = ChatConnectionState::Closed;
                return Err(::std::io::Error::new(ErrorKind::NotConnected, "No bytes read"));
            }

            // n bytes were read
            Ok(Some(n)) => {
                super::log_something(format!("read {} bytes", n));
                self.failed_read_attempts = 0;

                // The conditions have been met so that the input read from this connection
                // is now ready to be written to the other clients
                //
                // Limit is the number of characters up to the newline was detected, all characters after the newline are discarded.
                if let Some(limit) = self.is_ready_to_write() {

                    // Clear the current read buffer, but keep a handle to it since we will be returning it
                    // so that the server can add it to the other connection's send_queue's
                    let read_buf = mem::replace(&mut self.read_buf, Vec::new());

                    self.read_buf.truncate(limit);
                    return match String::from_utf8(read_buf) {
                        Ok(message) => {
                            return Ok(Some(message));
                        },
                        Err(_) => {
                            return Err(::std::io::Error::new(ErrorKind::InvalidInput, "Invalid utf8"));
                        }
                    }
                } else {
                    return Ok(None);
                }
            }
            // The socket's a liar! It wasn't actually ready for us to read from. 
            // Nothing we need to do here. Just keep listening same as before.
            Ok(None) => {
                self.failed_read_attempts = 0;

                return Ok(None);
            }
            Err(e) => {
                match e {
                    // Todo, determine what error kinds warrant retries, immediately closing the connection, ect...
                    // https://doc.rust-lang.org/std/io/enum.ErrorKind.html
                    // 
                    // For now just close the connection after 3 failed reads from the socket, regardless of the error type.
                    _ => {
                        self.failed_read_attempts += 1;
                        if self.failed_read_attempts > 3 {
                            self.state = ChatConnectionState::Closed;
                        }
                    }
                }
                return Err(e);
            }
        }
    }

    /// Writes to the connection, using the next entry from the send_queue.
    /// 
    /// Only the next entry in the send_queue will be sent per call. It may be better to just send 
    /// them all at once, separated by newlines.
    pub fn write(&mut self) -> io::Result<()> {
        let res = match self.send_queue.pop_front() {
            Some(buf) => {
                match self.socket.try_write_buf(&mut Cursor::new(buf.to_vec())) {
                    Ok(None) => {
                        super::log_something(format!("client flushing buf; WouldBlock"));

                        // Put message back into the queue so we can try again
                        self.failed_write_attempts += 1;
                        if self.failed_write_attempts > 3 {
                            self.state = ChatConnectionState::Closed;
                            Err(io::Error::new(io::ErrorKind::Other, "Exceeded failed write attempts limit."))
                        } else {
                            self.send_queue.push_front(buf);
                            Ok(())
                        }
                    },
                    Ok(Some(n)) => {
                        self.failed_write_attempts = 0;
                        super::log_something(format!("CONN : we wrote {} bytes", n));
                        Ok(())
                    },
                    Err(e) => {
                        super::log_something(format!("Failed to send buffer for {:?}, error: {}", self.token, e));
                        self.failed_write_attempts += 1;
                        if self.failed_write_attempts > 3 {
                            self.state = ChatConnectionState::Closed;
                        }

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
        self.send_queue.push_back(message);
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

    pub fn deregister(&mut self, event_loop: &mut mio::EventLoop<ChatServer>) -> io::Result<()> {
        event_loop.deregister(&self.socket)
    }

    pub fn quit(&mut self) {
        self.state = ChatConnectionState::Closed;
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
