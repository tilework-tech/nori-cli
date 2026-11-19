// Deterministic provider endpoints for the installed Claude and Codex binaries.
import assert from "node:assert/strict";

export const refused = "SYNTHETIC_NEVER_DELIVER_1704";
export const original = "SYNTHETIC_ORIGINAL_7041";
export const correction = "SYNTHETIC_CORRECTION_9352";
export const queued = "SYNTHETIC_NEXT_TURN_2810";

export const correctedReply = "Synthetic correction applied.";

function anthropicEvents(model, writePath, reply) {
  return [
    ["message_start", { message: { id: "msg_fixture", type: "message", role: "assistant", model,
      content: [], stop_reason: null, usage: { input_tokens: 1, output_tokens: 0 } } }],
    ["content_block_start", { index: 0, content_block: writePath
      ? { type: "tool_use", id: "toolu_synthetic", name: "Write", input: {} } : { type: "text", text: "" } }],
    ["content_block_delta", { index: 0, delta: writePath
      ? { type: "input_json_delta", partial_json: JSON.stringify({ file_path: writePath, content: "synthetic" }) }
      : { type: "text_delta", text: reply } }],
    ["content_block_stop", { index: 0 }],
    ["message_delta", { delta: { stop_reason: writePath ? "tool_use" : "end_turn", stop_sequence: null }, usage: { output_tokens: 1 } }],
    ["message_stop", {}],
  ];
}

function responsesEvents(ask, reply) {
  const message = ask ? {
    id: "fc_fixture", call_id: "call_fixture", type: "function_call", name: "exec_command",
    arguments: JSON.stringify({ cmd: "printf synthetic", sandbox_permissions: "require_escalated", justification: "Allow synthetic printf?" }),
    status: "completed",
  } : { id: "msg_fixture", type: "message", role: "assistant", status: "completed",
    content: [{ type: "output_text", text: reply, annotations: [] }] };
  return [
    ["response.created", { response: { id: "resp_fixture", status: "in_progress", output: [] } }],
    ["response.output_item.added", { output_index: 0, item: { ...message, content: [] } }],
    ...(!ask ? [["response.output_text.delta", { output_index: 0, content_index: 0, item_id: message.id, delta: reply }]] : []),
    ["response.output_item.done", { output_index: 0, item: message }],
    ["response.completed", { response: { id: "resp_fixture", status: "completed", output: [message],
      usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 } } }],
  ];
}

export function startModel(permissionPath) {
  let asked = false;
  const reached = Promise.withResolvers();
  const release = Promise.withResolvers();
  const corrected = Promise.withResolvers();
  const finishCorrection = Promise.withResolvers();
  const requests = [];
  const server = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    async fetch(request) {
      const path = new URL(request.url).pathname;
      if (request.method !== "POST" || !["/v1/messages", "/v1/responses"].includes(path)) {
        return Response.json({ data: [], models: [] });
      }
      let bytes = new Uint8Array(await request.arrayBuffer());
      if (request.headers.get("content-encoding") === "zstd") bytes = Bun.zstdDecompressSync(bytes);
      const body = JSON.parse(new TextDecoder().decode(bytes));
      const claude = path.endsWith("/messages");
      // Claude may send title requests without tools. Only count inference traffic.
      let ask = false;
      let reply = "Synthetic response.";
      if (!claude || body.tools?.length) {
        const input = JSON.stringify(claude ? body.messages : body.input);
        const counts = Object.fromEntries([original, correction, queued, refused].map(marker =>
          [marker, input.split(marker).length - 1]));
        requests.push(counts);
        if (!permissionPath && counts[correction] && !counts[queued]) {
          reply = correctedReply;
          corrected.resolve();
          await finishCorrection.promise;
        }
        if (counts[original]) {
          reached.resolve();
          if (permissionPath && !asked) {
            ask = true;
            asked = true;
          }
          if (!permissionPath) await release.promise;
        }
      }
      const events = claude ? anthropicEvents(body.model, ask && permissionPath, reply) : responsesEvents(ask, reply);
      const sse = events.map(([type, data]) =>
        'event: ' + type + '\ndata: ' + JSON.stringify({ ...data, type }) + '\n\n').join("");
      return new Response(sse, { headers: { "content-type": "text/event-stream" } });
    },
  });
  return { origin: server.url.origin, requests, reached: reached.promise,
    corrected: corrected.promise, finishCorrection: () => finishCorrection.resolve(),
    release: () => release.resolve(), close() { release.resolve(); finishCorrection.resolve(); server.stop(true); } };
}

export function checkDeliveries(requests) {
  // Provider requests include earlier history. Check each marker's multiplicity
  // within a request and the order of first appearance across requests.
  for (const request of requests) {
    assert.equal(request[refused], 0, 'refused input reached provider');
    assert.ok(request[correction] <= 1, 'duplicate correction');
    assert.ok(request[queued] <= 2, 'duplicate queued input');
  }
  const steered = requests.findIndex(request => request[correction] === 1);
  const next = requests.findIndex(request => request[queued] === 1);
  assert.ok(steered > 0, 'provider received the correction');
  assert.ok(next > steered, 'queue was sent only after steering');
  assert.ok(requests.slice(next + 1).some(request => request[queued] === 2), 'distinct queued IDs arrived on successive turns');
}
