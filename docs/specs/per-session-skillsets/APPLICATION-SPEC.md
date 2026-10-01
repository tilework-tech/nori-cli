We want to add support for using unique skillsets in different sessions.

User Journey A: No unique skillsets per session.
- The user starts a nori session.
- The user types a prompt and gets responses from the agent.
- Eventually, the user ends the session by typing /exit.

User Journey B: First time setup for unique skillsets per session.
- The user starts a nori session.
- The system checks to see if the skillsets-per-session field is set in the config.toml. It is not, so the system does not do anything.
- The user types /config
- Inside the /config menu, there is a setting called 'Per Session Skillsets'. It is defaulted to off.
- The user turns it on.
- The system checks to see if `nori-skillsets` is installed.
  - If not, the session shows a warning telling the user to install nori-skillsets, and otherwise does not do anything.
- If yes, the system updates the config.toml, setting the skillset-per-session field.
- The setting takes effect on the next session (see Notes; the session does not restart itself).
- The session checks if it is inside a git repository.
  - If not inside a git repository, nothing occurs.
  - If inside a git repository, it should behave as if the 'Auto Worktree' setting is on -- it should create a new worktree and run the branch name change.
- The session UI should then ask the user to pick a skillset from a list of available skillsets. It should populate the list by running `nori-skillsets list`, and then let the user select one by using the up and down arrows (or the j/k keys) and then hitting enter.
- Upon selection, the session should then run `nori-skillsets switch <selected skillset name> --install-dir <path/to/worktree>
- Finally, the cli should show the selected skillset in the status bar

User Journey C: Second+ time using unique skillsets.
- The user opens a nori session
- The system checks to see if the skillset-per-session field is set in the config.toml.
- It is, so the session checks if it is inside a git repository.
  - If not inside a git repository, nothing occurs.
  - If inside a git repository, it should behave as if the 'Auto Worktree' setting is on -- it should create a new worktree and run the branch name change.
- The session UI should then ask the user to pick a skillset from a list of available skillsets. It should populate the list by running `nori-skillsets list`, and then let the user select one by using the up and down arrows (or the j/k keys) and then hitting enter.
- Upon selection, the session should then run `nori-skillsets switch <selected skillset name> --install-dir <path/to/worktree>
- Finally, the cli should show the selected skillset in the status bar

User Journey D: Switching a unique skillset midsession
- The user opens a nori session
- The system checks to see if the skillset-per-session field is set in the config.toml.
- It is, so the session checks if it is inside a git repository.
  - If not inside a git repository, nothing occurs.
  - If inside a git repository, it should behave as if the 'Auto Worktree' setting is on -- it should create a new worktree and run the branch name change.
- The session UI should then ask the user to pick a skillset from a list of available skillsets. It should populate the list by running `nori-skillsets list`, and then let the user select one by using the up and down arrows (or the j/k keys) and then hitting enter.
- Upon selection, the session should then run `nori-skillsets switch <selected skillset name> --install-dir <path/to/worktree>
- Finally, the cli should show the selected skillset in the status bar
- The user has a conversation with the agent.
- At some point, the user types /switch-skillset
- A list of skillsets is populated using the `nori-skillsets list` command`
- The user selects a skillset
- The skillset is swapped _in the current worktree_ by running `nori-skillsets switch <selected skillset name> --install-dir <path/to/worktree>
- The cli should show the updated skillset in the status bar

Notes:
- The original plan was that skillset-per-session *requires* automatic worktrees and locks the auto-worktree option on. As built, it does not: enabling 'Per Session Skillsets' in /config opens a choice between "With Auto Worktrees" (also sets auto-worktree to automatic) and "Without Auto Worktrees". The setting is saved to config.toml and takes effect on the next session; the session does not restart itself.
- As built, the startup skillset picker opens whenever skillset-per-session is on (not in cloud mode), git repository or not. When the session cwd is not an auto-worktree (`.worktrees/<name>`), the selection runs `nori-skillsets install <name>` into the home install instead of `switch --install-dir`.

Implementation detail:
- Much of the individual pieces are already in place, such as automatic worktrees and session switching logic. Reuse those pieces, but refactor to centralized locations if necessary
- The CLI statusline should show the active skillset. The original plan read the nori-config.json 'activeSkillset' field and stored a session-local skillset name, because there was no direct way to view the active skillset of a local folder. That is superseded: `nori-skillsets list-active` now reports every skillset active in a directory and its parents, and both the footer and /status read its output. The session does not store its own skillset name; after a skillset install or switch it re-runs `list-active`. The 'activeSkillset' field is read only as a fallback for old nori-skillsets versions that do not support `list-active`.
