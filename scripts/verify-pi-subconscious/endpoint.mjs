// A loopback stand-in for Subconscious's OpenAI Chat Completions API.
// Each turn reads notes.txt, runs one shell command, then answers with what
// both tools returned. Every request is appended to the log as JSON.
import { appendFileSync } from "node:fs";

const MODELS = ["subconscious/glm-5.3-marathon", "subconscious/glm-5.2"];

function chunk(model, delta, finish = null) {
  return "data: " + JSON.stringify({
    id: "chatcmpl-synthetic", object: "chat.completion.chunk", created: 1795000000, model,
    choices: [{ index: 0, delta, finish_reason: finish }],
  }) + "\n\n";
}

function toolCall(model, id, name, args) {
  return chunk(model, { role: "assistant", tool_calls: [{ index: 0, id, type: "function", function: { name, arguments: JSON.stringify(args) } }] })
    + chunk(model, {}, "tool_calls");
}

function text(content) {
  return typeof content === "string" ? content : (content ?? []).map(part => part.text ?? "").join("");
}

export function startEndpoint(logPath, port = 0) {
  return Bun.serve({
    hostname: "127.0.0.1",
    port,
    async fetch(request) {
      const url = new URL(request.url);
      const body = request.method === "POST" ? await request.json() : null;
      appendFileSync(logPath, JSON.stringify({
        method: request.method, path: url.pathname,
        authorization: request.headers.get("authorization"),
        client: request.headers.get("x-subconscious-client"),
        model: body?.model, messages: body?.messages?.length ?? 0,
      }) + "\n");
      if (url.pathname.endsWith("/models")) {
        return Response.json({ object: "list", data: MODELS.map(id => ({ id, object: "model" })) });
      }
      if (!url.pathname.endsWith("/chat/completions")) return new Response("not found", { status: 404 });
      const tools = body.messages.filter(message => message.role === "tool").map(message => text(message.content));
      const model = body.model;
      let stream;
      if (tools.length === 0) {
        stream = toolCall(model, "call_read", "read", { path: "notes.txt" });
      } else if (tools.length === 1) {
        stream = toolCall(model, "call_bash", "bash", { command: "printf 'synthetic terminal: '; wc -l < notes.txt" });
      } else {
        const note = tools[0].match(/token=[\w-]+/)?.[0] ?? "no token";
        const terminal = tools[1].match(/synthetic terminal: *\d+/)?.[0] ?? "no terminal output";
        stream = chunk(model, { role: "assistant", content: `Read ${note}; ran ${terminal}.` })
          + chunk(model, {}, "stop");
      }
      stream += "data: " + JSON.stringify({ id: "chatcmpl-synthetic", object: "chat.completion.chunk", created: 1795000000, model, choices: [], usage: { prompt_tokens: 64, completion_tokens: 16, total_tokens: 80 } }) + "\n\ndata: [DONE]\n\n";
      return new Response(stream, { headers: { "content-type": "text/event-stream" } });
    },
  });
}

if (import.meta.main) {
  const server = startEndpoint(process.argv[2] ?? "/dev/null", Number(process.argv[3] ?? 0));
  console.log(`http://127.0.0.1:${server.port}/v1`);
}
