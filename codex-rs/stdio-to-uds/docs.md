# Noridoc: codex-stdio-to-uds

Path: @/codex-rs/stdio-to-uds

### Overview

The stdio-to-uds crate provides a bridge between standard input/output and Unix Domain Sockets. It relays bidirectional data between stdin/stdout and a UDS connection.

### How it fits into the larger codebase

Used for IPC scenarios where processes need to communicate via UDS while presenting a stdio interface. Supports both Unix and Windows (via `uds_windows` crate).

### Core Implementation

**run(socket_path)**: Main entry point that:
1. Connects to the UDS at `socket_path`
2. Spawns a thread to copy socket -> stdout
3. Copies stdin -> socket on main thread
4. Shuts down write side when stdin closes
5. Waits for stdout thread completion

The implementation uses blocking I/O with `std::io::copy` for simplicity.

### Things to Know

- Cross-platform: uses `std::os::unix::net::UnixStream` on Unix, `uds_windows::UnixStream` on Windows
- Blocking implementation - spawns a thread for the stdout direction
- Properly shuts down the socket write side to signal EOF to the peer
- Errors are contextualized with `anyhow::Context`

Created and maintained by Nori.
