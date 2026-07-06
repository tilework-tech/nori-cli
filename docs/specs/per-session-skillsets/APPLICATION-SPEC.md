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
- The session restarts itself.
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
- Because skillset-per-session *requires* automatic worktrees, the /config modal should show the automatic worktrees option as automatically enabled, and the user cannot disable it unless they first disable skillset-per-session (there should be message indicating this if the user tries to do so)

Implementation detail:
- Much of the individual pieces are already in place, such as automatic worktrees and session switching logic. Reuse those pieces, but refactor to centralized locations if necessary
- The CLI statusline should show the existing skillset. The way it does this right now has two problems.
  - First, it is out of date. It is looking inside the nori-config.json 'agents' field, but this no longer exists. It should instead look at the nori-config.json 'activeSkillset' field.
  - Second, even this approach will not work for the worktree-local skillset. Right now there is no direct way to view the currently active skillset in a local folder, so the nori session will just have to store that variable when in skillset-per-session mode
