# Noridoc: stdio-to-uds

Path: @/codex-rs/stdio-to-uds

### Overview

The `codex-stdio-to-uds` crate provides a utility for relaying stdin/stdout to a Unix domain socket. This enables process communication patterns where a subprocess needs to connect to a UDS server.

### How it fits into the larger codebase

Stdio-to-UDS is invoked via hidden CLI command:

- **CLI** `codex stdio-to-uds <SOCKET_PATH>` (hidden subcommand)
- **Bridges** stdio and Unix domain socket communication
- **Used** for IPC patterns

### Core Implementation

`run()` function:
1. Connects to specified Unix domain socket
2. Relays stdin to socket
3. Relays socket output to stdout
4. Handles bidirectional communication

### Things to Know

Useful for scenarios where a program expects stdio but needs to communicate with a UDS server.

Created and maintained by Nori.
