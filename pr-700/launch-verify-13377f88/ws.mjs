// Line bridge to a host's privileged /sessions control socket and /managed machine socket.
import { readFileSync } from "node:fs";
const home = process.argv[2];
const state = JSON.parse(readFileSync(`${home}/host/state.json`, "utf8"));
const token = readFileSync(`${home}/host/token`, "utf8").trim();
const urls = { control: state.endpoint, machine: state.endpoint.replace("/sessions", "/managed") };
const sockets = {};
function open(name) {
  if (sockets[name]) return sockets[name];
  const ws = new WebSocket(urls[name], { headers: { Authorization: `Bearer ${token}` } });
  const ready = new Promise((res, rej) => { ws.onopen = () => res(ws); ws.onerror = rej; });
  ws.onmessage = (e) => console.log(JSON.stringify({ from: name, msg: JSON.parse(e.data) }));
  ws.onclose = () => console.log(JSON.stringify({ from: name, closed: true }));
  return (sockets[name] = ready);
}
for await (const chunk of console) {
  const { to, msg } = JSON.parse(chunk);
  (await open(to)).send(JSON.stringify(msg));
}
