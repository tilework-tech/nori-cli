# #782 context

1. After a turn reporting 340,412 / 1,000,000 tokens, `/context` in the attached terminal lists /tmp/proof/srv/demo-a/AGENTS.md, a file only in the host session's folder (host runs a Codex-named mock agent so it loads AGENTS.md). 2. After detach, a terminal with other homes reattaches and the footer shows `34% / 1.00M` again.

Built from `feat/host-session-shell` at ef209c3e (every feature in the stack is present at that head). Mock agent, synthetic data. One background `nori host` with HOME=/tmp/proof/host-home and NORI_HOME=/tmp/proof/host-nori, sessions in /tmp/proof/srv/demo-a. Terminals run `nori attach --host` with their own HOME/NORI_HOME (/tmp/proof/client-home, /tmp/proof/client2-home) from /tmp/proof/work/client. Each frame folder holds screen.png (Ghostty Web 0.4.0 render), screen.txt, screen.ansi and metadata.json. Ghostty Web draws `↳` as a blank cell; screen.txt has it.
