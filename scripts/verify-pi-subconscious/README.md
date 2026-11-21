# Pi with Subconscious verification

Run from the repository root after building the candidate binary. The Bun client runs the real `pi-acp` adapter and Earendil's Pi coding agent through `nori exec`, against a loopback stand-in for Subconscious's OpenAI Chat Completions API. Use Bun and an npm cache that can supply both packages.

```sh
(cd nori-rs && cargo build --bin nori)
bun scripts/verify-pi-subconscious/verify.mjs nori-rs/target/debug/nori
```

The verifier creates a temporary home with a synthetic project and a `config.toml` that binds `pi` to a `subconscious` token provider whose `base_url` points at the loopback endpoint. It passes an explicit child environment and a synthetic key.

- Without `SUBCONSCIOUS_API_KEY`, `nori exec --agent pi` must exit unsuccessfully, name the variable, and send no request to the endpoint.
- With the key set, the endpoint drives three model turns: Pi reads `notes.txt`, runs a shell command that counts its lines, then answers with both results. The client checks the final answer, that every Chat Completions request carries the bearer key, the `x-subconscious-client: pi` header and the saved model, and that the key appears in neither `config.toml` nor the generated `models.json`.

`endpoint.mjs` can also run alone to serve the stand-in API: `bun scripts/verify-pi-subconscious/endpoint.mjs <log file> [port]` prints its base URL and appends one JSON line per request to the log.

## Running without network access

After one online run has filled the npm cache, the check can run in a loopback-only network namespace so no traffic can reach a real provider:

```sh
npm_config_offline=true unshare -rn sh -c \
  'ip link set lo up && bun scripts/verify-pi-subconscious/verify.mjs nori-rs/target/debug/nori'
```

The verifier forwards `npm_config_offline` and `npm_config_cache` to the child so `npx` resolves both packages from the cache.
