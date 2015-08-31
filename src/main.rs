#![feature(convert)]
#![feature(ip_addr)]
#![allow(unused_must_use)]

extern crate mio;
extern crate bytes;
extern crate time;

mod chat_server;

pub fn main() {
    chat_server::run_server("0.0.0.0:6567".parse().unwrap());
}