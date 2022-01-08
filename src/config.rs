use std::collections::HashMap;
use std::fs::read_to_string;

use json::parse;
use json::JsonValue;
use json::object::Object;

pub struct Config {
	pub files: HashMap<String, (String, String)>,
	pub address: String,
	pub directories: HashMap<String, (String, String)>,
	pub not_found: String,
	pub server: String,
	pub commands: HashMap<String, (String, Vec<String>)>,
}

fn get_str(obj: &Object, key: &str) -> String {
	match obj.get(key) {
		Some(JsonValue::String(s)) => s.clone(),
		Some(JsonValue::Short(s)) => String::from(s.as_str()),
		_ => panic!("cfg: `{}` must contain a string", key),
	}
}

fn json_to_str(v: &JsonValue) -> String {
	match v {
		JsonValue::String(s) => s.clone(),
		JsonValue::Short(s) => String::from(s.as_str()),
		_ => panic!("cfg: bad command/files format: {:?}", v),
	}
}

impl Config {
	pub fn import(path: &str) -> Self {
		let config = match parse(&read_to_string(path).expect("cfg: could not read file")) {
			Ok(JsonValue::Object(config)) => config,
			Ok(_) => panic!("cfg: file must represent an object"),
			Err(e) => panic!("cfg: invalid file; {}", e),
		};
		let mut files = HashMap::new();
		match config.get("files") {
			Some(JsonValue::Object(file_assocs)) => {
				for (k, v) in file_assocs.iter() {
					match v {
						JsonValue::Array(a) => {
							let mut c = a.iter().map(json_to_str).collect::<Vec<String>>();
							let mime = c.pop().expect("cfg: bad files format");
							let path = c.pop().expect("cfg: bad files format");
							files.insert(String::from(k), (path, mime));
						},
						_ => panic!("cfg: `{}`: path must be a string", k),
					};
				}
			},
			_ => panic!("cfg: missing `files` property"),
		}
		let mut directories = HashMap::new();
		match config.get("directories") {
			Some(JsonValue::Object(dir_assocs)) => {
				for (k, v) in dir_assocs.iter() {
					match v {
						JsonValue::Array(a) => {
							let mut c = a.iter().map(json_to_str).collect::<Vec<String>>();
							let mime = c.pop().expect("cfg: bad directories format");
							let path = c.pop().expect("cfg: bad directories format");
							directories.insert(String::from(k), (path, mime));
						},
						_ => panic!("cfg: `{}`: path must be a string", k),
					};
				}
			},
			_ => panic!("cfg: missing `directories` property"),
		}
		let mut commands = HashMap::new();
		match config.get("commands") {
			Some(JsonValue::Object(cmd_assocs)) => {
				for (k, v) in cmd_assocs.iter() {
					match v {
						JsonValue::Array(a) => {
							let mut c = a.iter().map(json_to_str).collect::<Vec<String>>();
							let args = c.split_off(1);
							let cmd = c.pop().expect("cfg: bad command format");
							commands.insert(String::from(k), (cmd, args));
						},
						_ => panic!("cfg: `{}`: path must be a string", k),
					}
				}
			}
			_ => panic!("cfg: missing `commands` property"),
		}
		Self {
			files,
			directories,
			address: get_str(&config, "address"),
			not_found: get_str(&config, "not_found"),
			server: get_str(&config, "server"),
			commands,
		}
	}
}
