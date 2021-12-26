# webgate

This command line utility allows you to:
- serve static files listed in a config file
- remotely run shell commands listed in a config file

The command's output is piped via websockets to the client as the command runs.
You can write to the command's input pipe via the same socket at any time.

### Configuration

Here is an example config file:

```json
{
	"address": "127.0.0.1:9001",
	"files": {
		"/index": ["static/index.html", "text/html"],
		"/favicon.png": ["static/favicon.png", "image/png"]
	},
	"not_found": "static/not_found.html",
	"server": "basmati",
	"commands": {
		"/pwd": ["pwd"],
		"/ls": ["ls", "-lha"],
		"/sh": ["sh"]
	}
}
```

This would expose two files over HTTP on 127.0.0.1:9001, as well as three commands.

Notes:
- The `server` key is what will be used as HTTP "Server" Response Header.
- The keys in `files` and `commands` must begin with a slash (`/`) so that they can be used to match the path in HTTP requests.

### Example command run code

```js
const CLIENT_READY = String.fromCharCode(0);
const CLIENT_KILL  = String.fromCharCode(1);
const CLIENT_PUSH  = String.fromCharCode(2);
const R_TYPES = {
	0: "fail", // the server could not start the subprocess
	1: "exit", // the subprocess exited
	2: "sout", // the process wrote new bytes to its standard output
	3: "serr"  // the process wrote new bytes to its standard error output
};

// Run the `/sh` command
let c = new WebSocket("ws://localhost:9001/sh");
c.onmessage = (e) => {
  let type = e.data.charCodeAt(0);
  let data = e.data.substring(1);
  if (type > 1) { // stdout has new bytes
    console.log(data); // print them to the console
  } else  {
    console.log("--- end of subprocess ---");
  }
}
c.send(CLIENT_READY);

// when you want to write something to to subprocess's stdin:
c.send(CLIENT_PUSH + "echo $USER\n");

// when you want to kill the subprocess
c.send(CLIENT_KILL);
```
