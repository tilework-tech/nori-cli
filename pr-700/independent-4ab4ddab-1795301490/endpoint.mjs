// Independent loopback stand-in for an OpenAI Chat Completions gateway.
// Usage: bun endpoint.mjs <log.jsonl> <marker-file-name>
// Logs per request: path, model, client header, SHA-256 of the Authorization header, tool names, tool-result count.
import { appendFileSync } from "node:fs";
import { createHash } from "node:crypto";
const [logPath, markerFile] = process.argv.slice(2);
const sha = s => s == null ? null : createHash("sha256").update(s).digest("hex");
const sse = o => "data: " + JSON.stringify(o) + "\n\n";
const chunk = (model, delta, finish = null) => sse({ id: "c-indep", object: "chat.completion.chunk", created: 1, model, choices: [{ index: 0, delta, finish_reason: finish }] });
const call = (model, id, name, args) => chunk(model, { role: "assistant", tool_calls: [{ index: 0, id, type: "function", function: { name, arguments: JSON.stringify(args) } }] }) + chunk(model, {}, "tool_calls");
const text = c => typeof c === "string" ? c : (c ?? []).map(p => p.text ?? "").join("");
const server = Bun.serve({ hostname: "127.0.0.1", port: Number(process.env.PORT ?? 0), async fetch(req) {
  const url = new URL(req.url);
  const body = req.method === "POST" ? await req.json().catch(() => null) : null;
  const toolMsgs = (body?.messages ?? []).filter(m => m.role === "tool").map(m => text(m.content));
  const userText = (body?.messages ?? []).filter(m => m.role === "user").map(m => text(m.content)).join(" ");
  appendFileSync(logPath, JSON.stringify({ t: new Date().toISOString(), method: req.method, path: url.pathname, model: body?.model ?? null,
    client: req.headers.get("x-subconscious-client"), authSha256: sha(req.headers.get("authorization")),
    headerNames: [...req.headers.keys()].sort(), tools: (body?.tools ?? []).map(t => t.function?.name), toolResults: toolMsgs.length,
    isTitleOrAux: !(body?.tools?.length) }) + "\n");
  if (url.pathname.endsWith("/models")) return Response.json({ object: "list", data: [] });
  if (!url.pathname.endsWith("/chat/completions")) return new Response("nf", { status: 404 });
  const model = body.model; let out;
  if (!body.tools?.length) out = chunk(model, { role: "assistant", content: "Synthetic title" }) + chunk(model, {}, "stop");
  else if (toolMsgs.length === 0) out = call(model, "c1", "read", { path: markerFile });
  else if (toolMsgs.length === 1) out = call(model, "c2", "bash", { command: `wc -l < ${markerFile} | tr -d '[:space:]'` });
  else {
    const marker = toolMsgs[0].match(/MARKER-[0-9a-f]+/)?.[0] ?? "no-marker";
    const lines = toolMsgs[1].match(/\d+/)?.[0] ?? "no-count";
    out = chunk(model, { role: "assistant", content: `marker ${marker}; lines ${lines}` }) + chunk(model, {}, "stop");
  }
  out += sse({ id: "c-indep", object: "chat.completion.chunk", created: 1, model, choices: [], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } }) + "data: [DONE]\n\n";
  return new Response(out, { headers: { "content-type": "text/event-stream" } });
} });
console.log(`http://127.0.0.1:${server.port}/v1`);
