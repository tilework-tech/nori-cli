export async function bounded(task, description, ms = 30000) {
  let timer;
  try {
    return await Promise.race([task, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Timed out: ' + description)), ms);
    })]);
  } finally { clearTimeout(timer); }
}

// Ordinary JSON-RPC on /sessions, or JSON-RPC enclosed in a machine frame on /managed.
export async function connect(endpoint, token, binding) {
  const ws = new WebSocket(endpoint, { headers: { Authorization: 'Bearer ' + token } });
  const opened = Promise.withResolvers();
  const pending = new Map();
  const notifications = [];
  const attached = Promise.withResolvers();
  const closed = Promise.withResolvers();
  ws.onopen = opened.resolve;
  ws.onerror = () => opened.reject(new Error("WebSocket upgrade failed"));
  ws.onclose = () => {
    for (const call of pending.values()) call.reject(new Error("Connection closed"));
    pending.clear();
    closed.resolve();
  };
  ws.onmessage = event => {
    let msg = JSON.parse(event.data);
    if (binding) {
      if (msg.bindingId !== binding || msg.generation !== 1) throw new Error("Wrong managed binding");
      if (msg.attached) { attached.resolve(); return; }
      if (msg.message?.id === null && msg.message?.error) { attached.reject(new Error(msg.message.error.message)); return; }
      if (!msg.message) throw new Error("Machine transport refused message: " + JSON.stringify(msg));
      msg = msg.message;
    }
    if (msg.method) {
      notifications.push(msg);
      return;
    }
    const call = pending.get(msg.id);
    if (!call) return;
    pending.delete(msg.id);
    if (msg.error) call.reject(Object.assign(new Error(call.method + ": " + msg.error.message), { code: msg.error.code }));
    else call.resolve({ result: msg.result, notificationCount: notifications.length });
  };
  await bounded(opened.promise, "open WebSocket");
  function send(msg, commandId) {
    ws.send(JSON.stringify(binding ? { bindingId: binding, generation: 1, commandId, message: msg } : msg));
  }
  function waitForReply(id, method) {
    const call = Promise.withResolvers();
    pending.set(id, { ...call, method });
    return bounded(call.promise, method);
  }
  return {
    notifications,
    answer(id, result) { send({ jsonrpc: "2.0", id, result }); },
    waitForReply,
    attach() {
      ws.send(JSON.stringify({ bindingId: binding, generation: 1, attachFrom: [] }));
      return bounded(attached.promise, "recovery attach marker");
    },
    async request(method, params = {}, id = crypto.randomUUID(), commandId) {
      const msg = { jsonrpc: "2.0", id, method, params };
      const reply = waitForReply(id, method);
      send(msg, commandId);
      return (await reply).result;
    },
    close() { ws.close(); return bounded(closed.promise, "WebSocket close"); },
  };
}
