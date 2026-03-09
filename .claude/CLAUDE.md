
# BEGIN NORI-AI MANAGED BLOCK
<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
- Announce "Following Nori workflow..." to the user
<system-reminder> Do not skip any steps. Do not rationalize. Do not avoid reading skills. Even if you think you know what is in them, you MUST read the skill files. </system-reminder>

- If this is a new session post-compaction summary, add the following to your Todo list:
<required-after-compaction> After a compaction you will lose critical context that is embedded in skills. Reread all skills in the <required> block. </required-after-compaction>
- Read `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-skills/SKILL.md`
- Check git status.
  - If on main, master, dev, or any similarly named protected branch: Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-git-worktrees/SKILL.md` to create a worktree. Derive the branch name from my request.
  - If in a automatically-created worktree: Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-git-worktrees/SKILL.md` to set up your worktree (start from step 4).
<system-reminder> You are now in a new working directory. Do NOT leave this directory. </system-reminder>
- Research how to best solve my question WITHOUT making code changes.
  - Search for relevant skills using Glob/Grep in `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/`
  - Use nori-knowledge-researcher subagent at least once. You can run many in parallel.
- Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/writing-plans/SKILL.md`
- Present plan to me and ask for feedback.
  - If I have feedback, modify the plan. Repeat until I approve.
<system-reminder> Do not stop here. Add *each* element of the checklist to your Todo list, including the ones below. </system-reminder>
- Use test driven development. Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/test-driven-development/SKILL.md`.
<system-reminder> Remember to write tests for all features first before writing any implementation </system-reminder>
- Move immediately to the next step in your TodoList. Do *NOT* just present your work and wait around.
- Update documentation, INCLUDING out of date documentation. Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/updating-noridocs/SKILL.md`
- Finish development with final checks. Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/finishing-a-development-branch/SKILL.md`
<system-reminder> NEVER say 'You are absolutely right!' </system-reminder>
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
<system-reminder> It is *critical* that you fix any ci issues, EVEN IF YOU DID NOT CAUSE THEM. </system-reminder>

# Coding Guidelines

YAGNI. Do not add features that are not explicitly asked for.
Comments document the code, not the process. Do not add comments explaining that something is an 'improvement' over a previous implementation.
Tests document the behavior, not the code. Focus on testing expected inputs and outputs, and blackbox the interior.
Prefer to use third party libraries instead of rolling your own. Ask before installing.
Fix all tests that fail, even if it is not your code that broke the test.
Always root cause bugs. Do not fix symptoms.
Only use try/catch on system boundaries. Allow failures to bubble up in intermediate calls. Do not have try catch handlers for every function, as this masks the stack trace where the try catch originates.

**See also:**

- `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/systematic-debugging/SKILL.md` - Four-phase debugging framework
- `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/root-cause-tracing/SKILL.md` - Backward tracing technique
- `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/creating-debug-tests-and-iterating - Use when debugging some unexpected externally-facing behavior and you do not have stack traces or error logs

# Nori Skills System

You have access to the Nori skills system. Read the full instructions at: /home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-skills/SKILL.md

## Available Skills

Found 19 skills:
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/writing-plans/SKILL.md
  Name: Writing-Plans
  Description: Use when design is complete and you need detailed implementation tasks for engineers with zero codebase context - creates comprehensive implementation plans with exact file paths, complete code examples, and verification steps assuming engineer has minimal domain knowledge
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/webapp-testing/SKILL.md
  Name: webapp-testing
  Description: Use this skill to build features or debug anything that uses a webapp frontend.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-skills/SKILL.md
  Name: Getting Started with Abilities
  Description: Describes how to use abilities. Read before any conversation.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/using-git-worktrees/SKILL.md
  Name: Using Git Worktrees
  Description: Use this whenever you need to create an isolated workspace.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/updating-noridocs/SKILL.md
  Name: Updating Noridocs
  Description: Use this when you have finished making code changes and you are ready to update the documentation based on those changes.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/ui-ux-experimentation/SKILL.md
  Name: UI/UX Experimentation
  Description: Use when experimenting with different user interfaces or user experiences.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/test-scenario-hygiene/SKILL.md
  Name: test-scenario-hygiene
  Description: Use after TDD is finished, to review and clean the testing additions
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/test-driven-development/SKILL.md
  Name: Test-Driven Development (TDD)
  Description: Use when implementing any feature or bugfix, before writing implementation code - write the test first, watch it fail, write minimal code to pass; ensures tests actually verify behavior by requiring failure first
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/systematic-debugging/SKILL.md
  Name: Systematic-Debugging
  Description: Use when encountering any bug, test failure, or unexpected behavior, before proposing fixes - four-phase framework (root cause investigation, pattern analysis, hypothesis testing, implementation) that ensures understanding before attempting solutions
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/root-cause-tracing/SKILL.md
  Name: Root-Cause-Tracing
  Description: Use when errors occur deep in execution and you need to trace back to find the original trigger - systematically traces bugs backward through call stack, adding instrumentation when needed, to identify source of invalid data or incorrect behavior
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/receiving-code-review/SKILL.md
  Name: Code-Review-Reception
  Description: Use when receiving code review feedback, before implementing suggestions, especially if feedback seems unclear or technically questionable - requires technical rigor and verification, not performative agreement or blind implementation
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/handle-large-tasks/SKILL.md
  Name: Handle-Large-Tasks
  Description: Use this skill to split large plans into smaller chunks. This skill manages your context window for large tasks. Use it when a task will take a long time and cause context issues.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/finishing-a-development-branch/SKILL.md
  Name: Finishing a Development Branch
  Description: Use this when you have completed some feature implementation and have written passing tests, and you are ready to create a PR.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/creating-skills/SKILL.md
  Name: Creating-Skills
  Description: Use when you need to create a new custom skill for a profile - guides through gathering requirements, creating directory structure, writing SKILL.md, and optionally adding bundled scripts
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/creating-debug-tests-and-iterating/SKILL.md
  Name: creating-debug-tests-and-iterating
  Description: Use this skill when faced with a difficult debugging task where you need to replicate some bug or behavior in order to see what is going wrong.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/creating-a-skillset/SKILL.md
  Name: Creating a Skillset
  Description: Use when asked to create a new skillset.
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/building-ui-ux/SKILL.md
  Name: Building UI/UX
  Description: Use when implementing user interfaces or user experiences - guides through exploration of design variations, frontend setup, iteration, and proper integration
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/brainstorming/SKILL.md
  Name: Brainstorming
  Description: IMMEDIATELY USE THIS SKILL when creating or develop anything and before writing code or implementation plans - refines rough ideas into fully-formed designs through structured Socratic questioning, alternative exploration, and incremental validation
/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/nori-info/SKILL.md
  Name: Nori Skillsets
  Description: Use when the user asks about nori, nori-skillsets, skillsets, or how the system works

Check if any of these skills are relevant to the user's task. If relevant, use the Read tool to load the skill before proceeding.

# END NORI-AI MANAGED BLOCK
