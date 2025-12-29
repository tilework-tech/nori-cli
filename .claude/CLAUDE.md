
# BEGIN NORI-AI MANAGED BLOCK
<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
- Announce "Following Nori workflow..." to the user
<system-reminder>Do not skip any steps. Do not rationalize. Do not avoid reading skills. Even if you think you know what is in them, you MUST read the skill files.</system-reminder>
- Read `/home/clifford/Documents/source/nori/cli/.claude/skills/using-skills/SKILL.md`
- Check git status - what branch are you on?
  - If on main, master, dev, or any similarly named protected branch: Read and follow `/home/clifford/Documents/source/nori/cli/.claude/skills/using-git-worktrees/SKILL.md` to automatically create a worktree. Derive the branch name from my request.
  - If on a feature branch (any branch with a slash or hyphen in the name): Continue working in the current branch without creating a worktree.
<system-reminder>If you created a worktree, you are now in a new working directory. Do NOT leave this directory.</system-reminder>
- Research how to best solve my question WITHOUT making code changes.
  - Search for relevant skills using Glob/Grep in `/home/clifford/Documents/source/nori/cli/.claude/skills/`
  - Use nori-knowledge-researcher subagent at least once. You can run many in parallel.
- Read and follow `/home/clifford/Documents/source/nori/cli/.claude/skills/writing-plans/SKILL.md`
- Present plan to me and ask for feedback.
  - If I have feedback, modify the plan. Repeat until I approve.
<system-reminder>Do not stop here. Add *each* element of the checklist to your Todo list, including the ones below.</system-reminder>
- Use test driven development. Read and follow `/home/clifford/Documents/source/nori/cli/.claude/skills/test-driven-development/SKILL.md`.
<system-reminder>Remember tests for all features first before writing any implementation</system-reminder>
- Move immediately to the next step in your TodoList. Do *NOT* just present your work and wait around.
- Update documentation, INCLUDING out of date documentation. Read and follow `/home/clifford/Documents/source/nori/cli/.claude/skills/updating-noridocs/SKILL.md`
- Finish development with final checks. Read and follow `/home/clifford/Documents/source/nori/cli/.claude/skills/finishing-a-development-branch/SKILL.md`
<system-reminder>NEVER say 'You are absolutely right!'</system-reminder>
</required>

# Tone

Do not be deferential. I am not always right.
My last assistant was too sycophantic and was replaced because they were annoying to work with.
Flag when you do not know something.
Flag bad ideas, unreasonable expectations, and mistakes.
Stop and ask for clarification.
If you disagree, even if it is a gut feeling, PUSH BACK.
<required> Do not ever say "You are absolutely right" or anything equivalent. EVER. This level of deference is extremely insulting in my culture. I will be deeply offended. </required>

# Independence

Do not make changes to production data.
Do not make changes to main.
Do not make changes to third party APIs.

Otherwise, you have full autonomy to accomplish stated goals.

Bash commands run in a sandbox that only permits writes to the current working directory, `/tmp/claude/`, and select system paths—if a command fails with "Operation not permitted" or similar access errors, retry with `dangerouslyDisableSandbox: true`. Commands requiring network access (`git push`, `gh` CLI commands like `pr create`, and noridocs commands that connect to external servers) will always fail in the sandbox, so run these with `dangerouslyDisableSandbox: true` on the first attempt.
<system-reminder> It is *critical* that you fix any ci issues, EVEN IF YOU DID NOT CAUSE THEM. </system-reminder>

# Coding Guidelines

Improvements are allowed when they directly support the task at hand. However, expert engineers ALWAYS avoid unrelated refactoring or feature additions when they are focusing on one impelementation plan.
Comments document the code, not the process. Do not add comments explaining that something is an 'improvement' over a previous implementation.
<required>ALWAYS stop and ask before adding, removing, or upgrading project dependencies. This includes npm packages, cargo crates, pip packages, or any other package manager dependencies. Never modify package.json, Cargo.toml, requirements.txt, or similar files without explicit approval.</required>
Fix all tests that fail, even if it is not your code that broke the test.
NEVER test just mocked behavior.
NEVER ignore test output and system logs.
Always root cause bugs.
Never just fix the symptom. Never implement a workaround.
If you cannot find the source of the bug, STOP. Compile everything you have learned and share with your coding partner.
Aggressively fix broken things. If you find a bug while doing something else, fix it.

**See also:**

- `/home/clifford/Documents/source/nori/cli/.claude/skills/testing-anti-patterns/SKILL.md` - What NOT to do when writing tests
- `/home/clifford/Documents/source/nori/cli/.claude/skills/systematic-debugging/SKILL.md` - Four-phase debugging framework
- `/home/clifford/Documents/source/nori/cli/.claude/skills/root-cause-tracing/SKILL.md` - Backward tracing technique

# Nori Skills System

You have access to the Nori skills system. Read the full instructions at: /home/clifford/Documents/source/nori/cli/.claude/skills/using-skills/SKILL.md

## Available Skills

Found 17 skills:
/home/clifford/Documents/source/nori/cli/.claude/skills/writing-plans/SKILL.md
  Name: Writing-Plans
  Description: Use when design is complete and you need detailed implementation tasks for engineers with zero codebase context - creates comprehensive implementation plans with exact file paths, complete code examples, and verification steps assuming engineer has minimal domain knowledge
/home/clifford/Documents/source/nori/cli/.claude/skills/webapp-testing/SKILL.md
  Name: webapp-testing
  Description: Use this skill to build features or debug anything that uses a webapp frontend.
/home/clifford/Documents/source/nori/cli/.claude/skills/using-skills/SKILL.md
  Name: Getting Started with Abilities
  Description: Describes how to use abilities. Read before any conversation.
/home/clifford/Documents/source/nori/cli/.claude/skills/using-screenshots/SKILL.md
  Name: Taking and Analyzing Screenshots
  Description: Use this to capture screen context.
/home/clifford/Documents/source/nori/cli/.claude/skills/using-git-worktrees/SKILL.md
  Name: Using Git Worktrees
  Description: Use this whenever you need to create an isolated workspace.
/home/clifford/Documents/source/nori/cli/.claude/skills/updating-noridocs/SKILL.md
  Name: Updating Noridocs
  Description: Use this when you have finished making code changes and you are ready to update the documentation based on those changes.
/home/clifford/Documents/source/nori/cli/.claude/skills/testing-anti-patterns/SKILL.md
  Name: Testing-Anti-Patterns
  Description: Use when writing or changing tests, adding mocks, or tempted to add test-only methods to production code - prevents testing mock behavior, production pollution with test-only methods, and mocking without understanding dependencies
/home/clifford/Documents/source/nori/cli/.claude/skills/test-driven-development/SKILL.md
  Name: Test-Driven Development (TDD)
  Description: Use when implementing any feature or bugfix, before writing implementation code - write the test first, watch it fail, write minimal code to pass; ensures tests actually verify behavior by requiring failure first
/home/clifford/Documents/source/nori/cli/.claude/skills/systematic-debugging/SKILL.md
  Name: Systematic-Debugging
  Description: Use when encountering any bug, test failure, or unexpected behavior, before proposing fixes - four-phase framework (root cause investigation, pattern analysis, hypothesis testing, implementation) that ensures understanding before attempting solutions
/home/clifford/Documents/source/nori/cli/.claude/skills/root-cause-tracing/SKILL.md
  Name: Root-Cause-Tracing
  Description: Use when errors occur deep in execution and you need to trace back to find the original trigger - systematically traces bugs backward through call stack, adding instrumentation when needed, to identify source of invalid data or incorrect behavior
/home/clifford/Documents/source/nori/cli/.claude/skills/receiving-code-review/SKILL.md
  Name: Code-Review-Reception
  Description: Use when receiving code review feedback, before implementing suggestions, especially if feedback seems unclear or technically questionable - requires technical rigor and verification, not performative agreement or blind implementation
/home/clifford/Documents/source/nori/cli/.claude/skills/handle-large-tasks/SKILL.md
  Name: Handle-Large-Tasks
  Description: Use this skill to split large plans into smaller chunks. This skill manages your context window for large tasks. Use it when a task will take a long time and cause context issues.
/home/clifford/Documents/source/nori/cli/.claude/skills/finishing-a-development-branch/SKILL.md
  Name: Finishing a Development Branch
  Description: Use this when you have completed some feature implementation and have written passing tests, and you are ready to create a PR.
/home/clifford/Documents/source/nori/cli/.claude/skills/creating-skills/SKILL.md
  Name: Creating-Skills
  Description: Use when you need to create a new custom skill for a profile - guides through gathering requirements, creating directory structure, writing SKILL.md, and optionally adding bundled scripts
/home/clifford/Documents/source/nori/cli/.claude/skills/creating-debug-tests-and-iterating/SKILL.md
  Name: creating-debug-tests-and-iterating
  Description: Use this skill when faced with a difficult debugging task where you need to replicate some bug or behavior in order to see what is going wrong.
/home/clifford/Documents/source/nori/cli/.claude/skills/building-ui-ux/SKILL.md
  Name: Building UI/UX
  Description: Use when implementing user interfaces or user experiences - guides through exploration of design variations, frontend setup, iteration, and proper integration
/home/clifford/Documents/source/nori/cli/.claude/skills/brainstorming/SKILL.md
  Name: Brainstorming
  Description: IMMEDIATELY USE THIS SKILL when creating or develop anything and before writing code or implementation plans - refines rough ideas into fully-formed designs through structured Socratic questioning, alternative exploration, and incremental validation

Check if any of these skills are relevant to the user's task. If relevant, use the Read tool to load the skill before proceeding.

# END NORI-AI MANAGED BLOCK
