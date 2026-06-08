PR https://github.com/tilework-tech/nori-sessions/pull/830 and PR https://github.com/tilework-tech/nori-cli/pull/486 together add support for the `nori cloud` command, a cli-local command that allows a user to connect to and spin up a nori sessions (~/code/nori/nori-sessions/**) session.

We now want to improve the behavior of nori sessions out so that the product experience is more seamless.

These changes will require a PR on the nori-sessions repo AND the nori-cli repo.

1. When the user runs `nori cloud`, instead of automatically starting a new session, it should show a list of previous sessions and have an option to select a new one. If the user selects one of the previous sessions it should just resume that one on the remote machine.

2. Many of the nori-cli options are exclusively for client side behavior. That includes:
- most of the things under /settings
- many of the slash commands
- many of the other options like the worktree stuff and the skillset switching stuff.
These should all be default disabled when in cloud mode.

3. The user should be able to restart conversations that were originally started on slack or discord through the cli (nori cloud).
