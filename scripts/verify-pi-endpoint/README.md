# Verify Pi against a loopback model endpoint

The verifier uses the installed `pi-acp` adapter and Pi coding agent. A loopback Chat Completions service asks Pi to read `notes.txt` and count its lines with a terminal command. The reply must include the contents of the file and the count reported by the command.

## Named API connection

From the repository root, point the existing named-connection check at a built Nori binary. Bun and an npm cache containing both Pi packages are required.

```sh
npm_config_offline=true bun scripts/verify-pi-endpoint/verify.mjs nori-rs/target/debug/nori
```

The check exercises a missing key, the saved custom endpoint and model, and a successful turn using a synthetic credential.

## Local and managed launch definitions

Use Python 3, Bun, Node.js 22.20 or newer, and an installed copy of `pi-acp@0.0.35` together with `@earendil-works/pi-coding-agent@1.6.0`. Pass the directory containing both npm executable links. Run from the repository root on Linux or macOS.

```sh
python3 scripts/verify-pi-endpoint/launch_matrix.py \
  nori-rs/target/debug/nori /path/to/node_modules/.bin
```

The matrix runs plain, env-only, wrapped and inner-override definitions through `nori --agent fixture-pi exec --acp -C WORKSPACE`, the ordinary local host and a dedicated fixture-managed `nori host --stdio --inherit-env --launch`. Each case uses a fresh home, workspace and synthetic credentials.

- A Node preload observes the real adapter and Pi processes. The fixture checks literal argv, the selected inner executable, child environment fingerprints, and the contents and permissions of the caller-supplied runtime directory. The wrapper receives its own prefix, an explicit `--`, the server and every literal server argument. The registered inner executable records the arguments the adapter supplies to Pi for plain, env-only and wrapped launches. The inner-override case supplies a second executable using `PI_ACP_PI_COMMAND`, checks that only this executable ran, and verifies that no outer wrapper ran.
- Every case requires ACP initialization, session creation, an `end_turn` response and the expected file and terminal results. The matrix compares SHA-256 digests of normalized child inputs across entry points. Environment values used for verification and the loopback authorization header are recorded as fingerprints.
- After each case the verifier checks that the adapter process group and the observed Pi PIDs have gone. On Linux it acts as a child subreaper to collect exited descendants when the container's init does not reap them. The fixture owns and removes the prepared directories.

The managed fixture writes a launch definition and submits it at the host's existing admission boundary. Deployed Sessions provisioning and a live model provider have separate verification.
