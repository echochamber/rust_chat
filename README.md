#Rust Chat Server

### What is it?

A simple chat server you can connect to using tcp. Based on examples found [here](http://nbaksalyar.github.io/2015/07/10/writing-chat-in-rust.html) and [here](https://github.com/carllerche/mio/blob/getting-started/doc/getting-started.md).

### How to run it
1. Install [rust **Nightly**](https://www.rust-lang.org/install.html) (We need nightly because of the two features included at the top of main.rs).
2. Clone or download this repository.
3. cd rust_chat/
4. Run the app with this command: `cargo run`

### Interacting with a running server
1. Telnet in: `X.X.X.X PPPP` where X is the ip address in main.rs and PPPP is the port #.
2. If step 1 was successful it should ask you for a username. Type your username and press enter.
3. If step 2 was successful you should be able to chat with other people in the chat room now. You will be in the "default" room.
4. Chat with other people in the same room as you by typing a message and pressing enter.

### Commands
Commands are messages where the first character is a '/' followed by the command name. For examples '/rooms'.

Currently support commands are:

* `/rooms` list all the currently active rooms
* `/join ROOM_NAME` leaves your current room and joins another. If that room does not exist yet it is created.
* `/quit` to disconnect from the server