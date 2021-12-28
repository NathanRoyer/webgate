use std::net::TcpStream;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::RawFd;
use std::os::unix::io::AsRawFd;
use std::mem::swap;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;

use crate::ProtocolEvent::Continue;
use crate::ProtocolEvent::Refresh;
use crate::ProtocolEvent::Remove;
use crate::ProtocolEvent;
use crate::Protocol;
use popol::interest::Interest;
use popol::interest::READ;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use byteorder::BigEndian;

const CLIENT_READY: u8 = 0;
const CLIENT_KILL:  u8 = 1;
const CLIENT_PUSH:  u8 = 2;

const PROCESS_FAIL: u8 = 0;
const PROCESS_EXIT: u8 = 1;
const PROCESS_SOUT: u8 = 2;
const PROCESS_SERR: u8 = 3;

pub struct WsSession {
	cmd: String,
	args: Vec<String>,
	stream: TcpStream,
	stream_buffer: Vec<u8>,
	prev_ws_fragments: Vec<u8>,
	process: Option<Child>,
	dead: bool,
}

pub fn spawn(cmd: &str, args: &[String]) -> Option<Child> {
	let mut child = Command::new(cmd);
	child.args(args);
	child.stdout(Stdio::piped());
	child.stderr(Stdio::piped());
	child.stdin(Stdio::piped());
	child.spawn().ok()
}

impl WsSession {
	pub fn new(stream: TcpStream, cmd: &(String, Vec<String>)) -> Self {
		Self {
			cmd: cmd.0.clone(),
			args: cmd.1.clone(),
			stream,
			stream_buffer: Vec::with_capacity(1024),
			prev_ws_fragments: Vec::new(),
			process: None,
			dead: false,
		}
	}

	pub fn parse_ws_frame(&mut self) -> Option<(bool, u8, Vec<u8>)> {
		let b0 = self.stream_buffer.get(0)?;
		let b1 = self.stream_buffer.get(1)?;
		let fin = (b0 >> 7) == 1;
		let opcode = b0 & 0b1111;
		let mask_length = ((b1 >> 7) * 4) as usize;
		let mut len = (b1 & 0b1111111) as usize;
		let to_len = 2;
		let ext_len_bytes = {
			let mut rdr = &self.stream_buffer[to_len..];
			match len {
				127 => (8, len = rdr.read_u64::<BigEndian>().ok()? as usize).0,
				126 => (2, len = rdr.read_u16::<BigEndian>().ok()? as usize).0,
				_ => 0,
			}
		};
		let to_mask = to_len + ext_len_bytes;
		let to_payload = to_mask + mask_length;
		if self.stream_buffer.len() >= (to_payload + len) {
			let mut fragment = self.stream_buffer.split_off(to_payload);
			let mut header = fragment.split_off(len);
			// placing next frame in stream_buffer and getting header back:
			swap(&mut self.stream_buffer, &mut header);
			if mask_length > 0 {
				for i in 0..len {
					let b = fragment[i];
					let k = header[to_mask + (i & 0b11)];
					fragment[i] = b ^ k;
				}
			}
			Some((fin, opcode, fragment))
		} else {
			None
		}
	}

	fn ws_frame_header(&self, fin: bool, opcode: u8, len: usize) -> Vec<u8> {
		let b0 = (opcode & 0b1111) | ((fin as u8) << 7);
		let (b1, ext_len_bytes) = match len {
			0..=125 => (len as u8, 0),
			126..=0xffff => (126, 2),
			_ =>            (127, 8),
		};
		// masked bit is 0 because we're serving.
		let mut header = vec![b0, b1];
		match ext_len_bytes {
			8 => header.write_u64::<BigEndian>(len as u64).unwrap(),
			2 => header.write_u16::<BigEndian>(len as u16).unwrap(),
			_ => (),
		}
		header
	}

	fn process_ws_message(&mut self, msg: &[u8], retval: &mut ProtocolEvent) {
		match (msg[0], self.process.is_some()) {
			(CLIENT_READY, false) => {
				match spawn(&self.cmd, &self.args) {
					Some(child) => self.process = Some(child),
					None => self.send_ws_message(&[PROCESS_FAIL]),
				}
				*retval = Refresh;
			},
			(CLIENT_KILL, true) => {
				let _ = self.process.as_mut().unwrap().kill();
			},
			(CLIENT_PUSH, true) => {
				let pipe = self.process.as_ref().unwrap().stdin.as_ref();
				let _ = pipe.unwrap().write(&msg[1..]);
			},
			_ => println!("process: bad message"),
		}
	}

	fn send_ws_pong(&mut self, data: &[u8]) {
		let mut frame = self.ws_frame_header(true, 10, data.len());
		frame.extend_from_slice(data);
		let _ = self.stream.write(&frame);
	}

	fn send_ws_message(&mut self, data: &[u8]) {
		let mut frame = self.ws_frame_header(true, 1, data.len());
		frame.extend_from_slice(data);
		let _ = self.stream.write(&frame);
	}

	fn process_exit(&mut self) -> ProtocolEvent {
		self.dead = true;
		let mut msg = vec![PROCESS_EXIT];
		if let Ok(Some(status)) = self.process.as_mut().unwrap().try_wait() {
			if let Some(code) = status.code() {
				msg.extend_from_slice(format!("{}", code).as_bytes());
			}
		}
		self.send_ws_message(&msg);
		Refresh
	}
}

impl Protocol for WsSession {
	fn pollfds(&self) -> Vec<(RawFd, Interest)> {
		let mut fds = vec![(self.stream.as_raw_fd(), READ)];
		if !self.dead {
			if let Some(process) = &self.process {
				let stdout = process.stdout.as_ref().unwrap();
				let stderr = process.stderr.as_ref().unwrap();
				fds.push((stdout.as_raw_fd(), READ));
				fds.push((stderr.as_raw_fd(), READ));
			}
		}
		fds
	}

	fn incoming(&mut self, fd: usize) -> ProtocolEvent {
		let mut ret_val = Continue;
		if fd == 0 {
			let mut bytes = [0u8; 1024];
			let len = match self.stream.read(&mut bytes) {
				Ok(len) if len > 0 => len,
				_ => return Remove,
			};
			self.stream_buffer.extend_from_slice(&bytes[0..len]);
			while let Some((fin, opcode, fragment)) = self.parse_ws_frame() {
				self.prev_ws_fragments.extend_from_slice(&fragment);
				if fin {
					let mut data = Vec::new();
					swap(&mut self.prev_ws_fragments, &mut data);
					match opcode {
						0x8 => return Remove,
						0x9 => self.send_ws_pong(&data),
						1|2 => self.process_ws_message(&data, &mut ret_val),
						_ => (),
					}
				}
			}
		} else if fd == 1 {
			let pipe = self.process.as_mut().unwrap().stdout.as_mut().unwrap();
			let mut bytes = [PROCESS_SOUT; 1024];
			let len = match pipe.read(&mut bytes[1..]) {
				Ok(len) if len > 0 => len,
				_ => return self.process_exit(),
			};
			self.send_ws_message(&bytes[0..len + 1]);
		} else {
			let pipe = self.process.as_mut().unwrap().stderr.as_mut().unwrap();
			let mut bytes = [PROCESS_SERR; 1024];
			let len = match pipe.read(&mut bytes[1..]) {
				Ok(len) if len > 0 => len,
				_ => return self.process_exit(),
			};
			self.send_ws_message(&bytes[0..len + 1]);
		}
		ret_val
	}
}
