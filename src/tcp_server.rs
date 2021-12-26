use std::net::TcpListener;
use std::os::unix::io::RawFd;
use std::os::unix::io::AsRawFd;

use crate::ProtocolEvent::AddSibling;
use crate::ProtocolEvent::Error;
use crate::ProtocolEvent;
use crate::Protocol;
use popol::interest::Interest;
use popol::interest::READ;

use crate::http::HttpSession;

pub struct TcpServer {
	listener: TcpListener,
}

impl TcpServer {
	pub fn new(address: &str) -> Self {
		Self {
			listener: TcpListener::bind(address).unwrap(),
		}
	}
}

impl Protocol for TcpServer {
	fn pollfds(&self) -> Vec<(RawFd, Interest)> {
		vec![(self.listener.as_raw_fd(), READ)]
	}

	fn incoming(&mut self, _fd: usize) -> ProtocolEvent {
		match self.listener.accept() {
			Ok((socket, _addr)) => AddSibling(Box::new(HttpSession::new(socket))),
			Err(e) => Error(format!("couldn't accept client: {:?}", e)),
		}
	}
}
