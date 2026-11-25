# #779 prompt history

Terminal A sent two prompts and detached. Terminal B (other HOME and NORI_HOME, whose NORI_HOME holds no history.jsonl) opens Ctrl+R: 1. lists both prompts from the host's history; 2. `/walnut` finds the matching one.

Built from `feat/host-session-shell` at ef209c3e (every feature in the stack is present at that head). Mock agent, synthetic data. One background `nori host` with HOME=/tmp/proof/host-home and NORI_HOME=/tmp/proof/host-nori, sessions in /tmp/proof/srv/demo-a. Terminals run `nori attach --host` with their own HOME/NORI_HOME (/tmp/proof/client-home, /tmp/proof/client2-home) from /tmp/proof/work/client. Each frame folder holds screen.png (Ghostty Web 0.4.0 render), screen.txt, screen.ansi and metadata.json. Ghostty Web draws `↳` as a blank cell; screen.txt has it.
