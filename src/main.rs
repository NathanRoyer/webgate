use std::io::ErrorKind::TimedOut;
use std::time::Duration;
use std::os::unix::io::RawFd;
use std::env::args;

use popol::Sources;
use popol::Events;
use popol::interest::Interest;

use lazy_static::lazy_static;

pub mod http;
pub mod websockets;
pub mod tcp_server;
pub mod config;

use config::Config;
use tcp_server::TcpServer;
use http::load_from_config;

lazy_static! {
	pub static ref CONFIG: Config = Config::import(&args().last().unwrap());
}

type ProtocolBox = Box<dyn Protocol>;

pub enum ProtocolEvent {
	ReplaceWith(ProtocolBox),
	AddSibling(ProtocolBox),
	Remove,
	Refresh,
	Error(String),
	Continue,
}
use ProtocolEvent::*;

pub trait Protocol {
	fn pollfds(&self) -> Vec<(RawFd, Interest)>;
	fn incoming(&mut self, fd: usize) -> ProtocolEvent;
}

type Key = (usize, usize);

fn refresh_pollfds(pollfds: &mut Sources<Key>, clients: &Vec<ProtocolBox>) {
    *pollfds = Sources::<Key>::with_capacity(16);
    for c in 0..clients.len() {
    	let fds = clients[c].pollfds();
    	for i in 0..fds.len() {
    		let (fd, interest) = fds[i];
    		pollfds.register((c, i), &fd, interest);
    	}
    }
}

fn main() {
	let mut clients = Vec::<ProtocolBox>::with_capacity(16);
    let mut pollfds = Sources::<Key>::new();
    let mut events = Events::with_capacity(16);
    let server = TcpServer::new(&CONFIG.address);

    load_from_config();

    clients.push(Box::new(server));
    refresh_pollfds(&mut pollfds, &clients);

    println!("webgate: running");

	loop {
		match pollfds.wait_timeout(&mut events, Duration::from_secs(1)) {
			Err(err) if err.kind() != TimedOut => println!("webgate: {:?}", err),
			_ => (),
		}
		
		for ((c, fdi), _event) in events.iter() {
			let (c, fdi) = (*c, *fdi);
			let need_refresh = match clients[c].incoming(fdi) {
				ReplaceWith(p) => (clients[c] = p, true).1,
				AddSibling(p) => (clients.push(p), true).1,
				Remove => (clients.swap_remove(c), true).1,
				Error(e) => (println!("{}: {}", c, e), false).1,
				Refresh => true,
				Continue => false,
			};
			if need_refresh {
				refresh_pollfds(&mut pollfds, &clients);
			}
		}
	}
}
