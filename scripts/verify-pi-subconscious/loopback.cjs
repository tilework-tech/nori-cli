// Loaded only by the verifier's Node children. Keep the saved gateway URL intact.
const local = new URL(process.env.NORI_PI_TEST_ENDPOINT);
if (local.protocol !== "http:" || local.hostname !== "127.0.0.1") {
  throw new Error("The Pi verification endpoint must be on IPv4 loopback");
}
const fetchUpstream = globalThis.fetch;
globalThis.fetch = (input, init) => {
  const url = new URL(input instanceof Request ? input.url : input);
  if (url.origin !== "https://api.subconscious.dev") {
    return fetchUpstream(input, init);
  }
  const target = new URL(url.pathname + url.search, local.origin);
  return fetchUpstream(input instanceof Request ? new Request(target, input) : target, init);
};
