use std::net::TcpStream;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::RawFd;
use std::os::unix::io::AsRawFd;
use std::collections::HashMap;
use std::mem::swap;
use std::str::from_utf8;
use std::fs::read;

use crate::ProtocolEvent::ReplaceWith;
use crate::ProtocolEvent::Remove;
use crate::ProtocolEvent::Error;
use crate::ProtocolEvent::Continue;
use crate::ProtocolEvent;
use crate::Protocol;
use popol::interest::Interest;
use popol::interest::READ;

use crate::websockets::WsSession;
use crate::CONFIG;

use lazy_static::lazy_static;
use base64::encode;
use sha1::Sha1;

fn read_file(path: &str) -> Vec<u8> {
	read(path).expect(&format!("http: could not read {}", path))
}

fn resp(code: &'static str, content: &[u8], mime: &'static str) -> Vec<u8> {
	let len = content.len();
	let mut response = Vec::with_capacity(len + 100);
	response.extend_from_slice("HTTP/1.1 ".as_bytes());
	response.extend_from_slice(code.as_bytes());
	response.extend_from_slice("\r\n".as_bytes());
	response.extend_from_slice(format!("Content-Length: {}\r\n", len).as_bytes());
	response.extend_from_slice(format!("Content-Type: {}\r\n", mime).as_bytes());
	response.extend_from_slice(format!("Server: {}\r\n", &CONFIG.server).as_bytes());
	response.extend_from_slice("\r\n".as_bytes());
	response.extend_from_slice(content);
	response
}

fn ws_handshake(mut key: String) -> Vec<u8> {
	key += "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
	let resp_key = encode(Sha1::from(key).digest().bytes());
	let mut response = Vec::with_capacity(200);
	response.extend_from_slice(HANDSHAKE_PREFIX.as_bytes());
	response.extend_from_slice(resp_key.as_bytes());
	response.extend_from_slice(&DOUBLE_CRLF);
	response
}

lazy_static! {
	static ref RESOURCES: HashMap<String, Vec<u8>> = {
		let mut m = HashMap::new();
		for (name, (path, mime)) in &CONFIG.files {
			println!("http: preloading {}", path);
			m.insert(name.clone(), resp("200 OK", &read_file(path), mime));
		}
		m
	};
	static ref NOT_FOUND: Vec<u8> = resp("404 NOT FOUND", &read_file(&CONFIG.not_found), "text/html");
	static ref HANDSHAKE_PREFIX: String = {
		let mut s = String::from("HTTP/1.1 101 Switching Protocols\r\n");
		s += "Connection: Upgrade\r\n";
		s += "Upgrade: websocket\r\n";
		s += "Server: ";
		s += &CONFIG.server;
		s += "\r\n";
		s + "Sec-WebSocket-Accept: "
	};
}

const DOUBLE_CRLF: [u8; 4] = [13, 10, 13, 10];

pub fn load_from_config() {
	let _ = RESOURCES.get("");
	let _ = NOT_FOUND.get(0);
}

pub struct HttpSession {
	stream: Vec<TcpStream>,
	stream_buffer: Vec<u8>,
}

impl HttpSession {
	pub fn new(stream: TcpStream) -> Self {
		Self {
			stream: vec![stream],
			stream_buffer: Vec::with_capacity(1024),
		}
	}

	fn check_http_header_received(&mut self) -> Option<(String, Option<String>)> {
		let p = self.stream_buffer.windows(4).position(|s| s == DOUBLE_CRLF)?;
		let mut prefix = self.stream_buffer.split_off(p + 4);
		swap(&mut self.stream_buffer, &mut prefix);
		let headers = from_utf8(&prefix[0..p]).ok()?;
		let mut lines = headers.split("\r\n");
		let path = {
			let mut first_line = lines.next()?.split(' ');
			let _method = first_line.next()?;
			String::from(first_line.next()?)
		};
		let mut ws_key = None;
		for line in lines {
			let mut first_line = line.split(": ");
			let hname = first_line.next()?.to_lowercase();
			let value = first_line.next()?.to_string();
			if hname == "sec-websocket-key" {
				ws_key = Some(value);
			}
		}
		Some((path, ws_key))
	}
}

impl Protocol for HttpSession {
	fn pollfds(&self) -> Vec<(RawFd, Interest)> {
		vec![(self.stream[0].as_raw_fd(), READ)]
	}

	fn incoming(&mut self, _fd: usize) -> ProtocolEvent {
		let mut bytes = [0u8; 1024];
		let len = match self.stream[0].read(&mut bytes) {
			Ok(len) if len > 0 => len,
			_ => return Error(String::from("http session: cound not read")),
		};
		self.stream_buffer.extend_from_slice(&bytes[0..len]);
		let (path, ws) = match self.check_http_header_received() {
			Some(tuple) => tuple,
			None => return Continue,
		};
		println!("http: {}://localhost{}", ["http", "ws"][ws.is_some() as usize], &path);
		if let Some(key) = ws {
			// pending ws upgrade
			if let Some(cmd) = CONFIG.commands.get(&path) {
				let mut stream = self.stream.pop().unwrap();
				let _ = stream.write(&ws_handshake(key));
				ReplaceWith(Box::new(WsSession::new(stream, cmd)))
			} else {
				let _ = self.stream[0].write(&NOT_FOUND);
				Remove
			}
		} else {
			// static http service
			let _ = self.stream[0].write(match RESOURCES.get(path.as_str()) {
				Some(bytes) => bytes,
				None => &NOT_FOUND,
			});
			Remove
		}
	}
}
