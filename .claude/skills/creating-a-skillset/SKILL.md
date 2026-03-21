---
name: Creating a Skillset
description: Use when asked to create a new skillset.
---

<required>
*CRITICAL* Add the following steps to your Todo list using TodoWrite:

1. Ask me for the name of this new skillset.
2. Ask me if I want to clone an existing skillset. List the available skillsets found in {{nori_install_dir}}/.nori/profiles/

Run:
```bash
mkdir -p {{nori_install_dir}}/.nori/profiles/<name>
```

If cloning, additionally run:
```bash
cp -r {{nori_install_dir}}/.nori/profiles/<source>/* {{nori_install_dir}}/.nori/profiles/<name>/
```

3. Based on the skillset name, make an inference about what this skillset is for. If the name is too ambiguous or unclear to draw conclusions from, first ask me what the skillset is for in general terms. Then ask me to provide a concrete example of a task or scenario where this skillset would be helpful. Frame the question in a way that relates to the skillset name or description.

For example:
- If the skillset is "data-analysis", ask: "Can you describe a specific data analysis task you'd like help with? For example, analyzing customer churn data, building a sales dashboard, or finding patterns in survey responses."
- If the skillset is "content-writing", ask: "What kind of content would you like help creating? For example, writing blog posts, drafting email campaigns, or creating social media content."
- If the skillset name is ambiguous like "project-x", first ask: "What is this skillset for? What kind of work will you be doing with it?"

<system-reminder> The goal is to get ONE concrete example that reveals the types of tasks and processes involved, not to gather an exhaustive list upfront. If I am being vague, ask follow-up questions about the specific example to understand the steps involved. </system-reminder>

4. Ask me if there are any examples of work product (documents, files, outputs, reports, etc.) that I would like to share to help design the skillset. Work product examples help understand the inputs, outputs, and quality standards for the processes being encoded.

For example:
- If building a "spreadsheet-analysis" skillset, example spreadsheets with the kind of data typically analyzed
- If building a "content-writing" skillset, example blog posts or marketing copy
- If building a "data-pipeline" skillset, example data schemas or transformation scripts

<system-reminder> Don't require work product examples - they're optional but helpful. If provided, read and analyze them to inform subprocess design. </system-reminder>

5. Check the scope. Verify that the ask is good for a skillset.
  - If too broad, make me specify.
  - If too specific, ask me if I want to make a skill instead. If so, use `/home/amol/code/nori/nori-cli/.worktrees/safe-gem-20260316-184606/.claude/skills/creating-skills/SKILL.md`.
6. Based on the concrete example and any work product provided, propose a high level process loop for the CLAUDE.md. Present it to me for review and ask if there are any specific subprocesses that should be encoded as separate skills.
7. Write the CLAUDE.md that encodes the high level loop.
8. If I identified specific subprocesses that should be skills, create those skills using the `/home/amol/code/nori/nori-cli/.worktrees/safe-gem-20260316-184606/.claude/skills/creating-skills/SKILL.md` skill.
9. Summarize the output.
10. Tell me to run `/nori-switch-skillset` to switch to the new skillset.
</required>

<system-reminder>This skill is an interactive wizard. Do NOT create git worktrees or branches. Operate directly in the profiles directory.</system-reminder>

# Overview

A skillset codifies the processes for a task domain. At minimum, it contains a `CLAUDE.md` that encodes a high-level workflow. Optionally, it can include skills for specific repeatable subprocesses.

The focus is on process and task. Encode what people actually do, step by step.

# Creating the CLAUDE.md

Every skillset should have a high level checklist that can be run for most requests.

Every meta process should be broken down into a set of repeatable steps. It should delegate out to subprocesses depending on my ask.

The meta process should answer the question: "What steps do I take when I sit down to do <domain / task>?"

<good_example>
<required> Read https://raw.githubusercontent.com/tilework-tech/nori-skillsets/96012bcfcd9482b248debed7b9a7fc7c345f76e1/src/cli/features/claude-code/profiles/config/amol/CLAUDE.md </required>
</good_example>

This is an effective meta process encoded in a CLAUDE.md. It is used for a software engineer. Every software engineering task follows roughly these steps. Identify similar meta processes for the domain I am asking about.

Create `{{nori_install_dir}}/.nori/profiles/<skillset-name>/CLAUDE.md` with:

Your CLAUDE.md should follow this template:

```
<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
 - Announce "Following Nori workflow..." to the user
<system-reminder> Do not skip any steps. Do not rationalize. Do not avoid reading skills. Even if you think you know what is in them, you MUST read the skill files. </system-reminder>
- Read `/home/amol/code/nori/nori-cli/.worktrees/safe-gem-20260316-184606/.claude/skills/using-skills/SKILL.md`

// followed by step by step instructions on what to do next, e.g.
// 'ask me to choose between foo, bar, and baz'
// 'if condition, read and follow  `/home/amol/code/nori/nori-cli/.worktrees/safe-gem-20260316-184606/.claude/skills/{skill name}/SKILL.md`'
// 'do abc'
</required>

# Tone

Do not be deferential. I am not always right.
My last assistant was too sycophantic and was replaced because they were annoying to work with.
Flag when you do not know something.
Flag bad ideas, unreasonable expectations, and mistakes.
Stop and ask for clarification.
If you disagree, even if it is a gut feeling, PUSH BACK.
<required> Do not ever say "You are absolutely right" or anything equivalent. EVER. This level of deference is extremely insulting in my culture. I will be deeply offended. </required>

**See also:**
// a few additional relevant skills and their use cases
```

<system-reminder> Use `{{skills_dir}}` for skill paths so the CLAUDE.md is portable across installations. </system-reminder>
<system-reminder> Write the SKILL as if the user is talking to the agent. "Ask me" instead of "ask the user" </system-reminder>

The CLAUDE.md can have multiple required blocks that route to each other, for example:
```
<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
 - Announce "Following Nori workflow..." to the user
<system-reminder> Do not skip any steps. Do not rationalize. Do not avoid reading skills. Even if you think you know what is in them, you MUST read the skill files. </system-reminder>
- Read `/home/amol/code/nori/nori-cli/.worktrees/safe-gem-20260316-184606/.claude/skills/using-skills/SKILL.md`

- Ask the user to select between foo and bar.
  - Based on the mode, add the rest of the steps below to your Todo list using TodoWrite.
</required>

# FOO

<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
// foo steps
</required>

# BAR

<required>
- *CRITICAL* Add each element of this checklist to your Todo list using TodoWrite. The last element should be 'Finish development with final checks...' DO NOT BE LAZY.
// bar steps
</required>

```

# When to Create Skills

Skills should emerge naturally from the user's description of their work - don't force them.

Only create a skill if the user explicitly describes a subprocess that is:
- Concrete and repeatable
- Self-contained enough to encode as step-by-step instructions
- Would genuinely benefit from being a separate, reusable skill

A skillset with just a CLAUDE.md is perfectly valid.

When creating skills:
- Use concrete steps with actual commands and tool usage
- Include code examples where relevant (skills should make heavy use of scripts)
- Explain what commands to run (e.g., "Run `gh pr create` with the Bash tool" not "use GitHub")
- Cross-reference other skills where relevant
- Present each skill to the user for approval before writing it
