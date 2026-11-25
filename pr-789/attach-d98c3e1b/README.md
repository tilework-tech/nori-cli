# #789 run ! commands only from terminals on the host's machine

Head: d98c3e1b (tilework-tech/nori-cli PR #789). Debug build of that tree, mock agent.

One background `nori host --port 0` with NORI_HOME=/tmp/pr789/hosthome, session folder /tmp/pr789/hosthome/project.
Terminal A runs plain `nori attach` with the host's NORI_HOME (same machine), its own HOME and folder (/tmp/pr789/acwd).
Terminal B runs `nori attach --host <endpoint> --token-file <host token>` with its own NORI_HOME, HOME and folder (/tmp/pr789/bcwd).

1. `1-local-runs-in-session-folder`: terminal A after `/new /tmp/pr789/hosthome/project` and `!pwd`. A `Ran pwd` cell prints the host session's folder, not A's own folder.
2. `2-remote-gets-gray-hint`: terminal B opened the session, took control with Alt+T, and submitted `!touch /tmp/pr789/marker/ran-remotely`. It shows the hint "Remote shell commands are not yet supported." in gray (SGR `38;5;8` in screen.ansi), the command is back in the prompt in shell mode (`! touch ...`), and there is no `Ran touch` cell.
3. `3-local-sees-no-new-ran-cell`: terminal A after B's attempt. It is now watching; the only command cell is the earlier `Ran pwd`.

`checks.txt` records `ls` showing the marker file was never created.

Each frame folder holds screen.png (Ghostty Web 0.4.0 render, 120x40), screen.txt, screen.ansi and metadata.json.

All data is synthetic.
