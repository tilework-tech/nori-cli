---
name: Creating-Skills
description: Use when you need to create a new custom skill for a profile - guides through gathering requirements, creating directory structure, writing SKILL.md, and optionally adding bundled scripts
---

<required>
*CRITICAL* Add the following steps to your Todo list using TodoWrite:

1. Gather skill requirements from me
2. Select target profile
3. Create skill directory structure
4. Write SKILL.md with proper frontmatter
5. (Optional) Write and bundle scripts
6. Instruct the user to run /nori-switch-profile to switch profiles.
</required>

# Overview

This skill guides you through creating custom skills that persist across sessions. Skills are stored in profile directories and can include markdown instructions, checklists, and optional bundled scripts.

# Writing Skills

Every skill must start with a required checklist block:

```
<required>
*CRITICAL* Add the following steps to your Todo list using TodoWrite:
1. <step 1>
2. <step 2>
...
</required>
```

This is the *most important* part of a skill.

Each step may have guidelines underneath. For example:
```
1. Create a directory.

Use `mkdir foo/bar`

2. Make a file.

...
```

# Writing scripts

Skills may be bundled with scripts. Scripts are simple code cli tools that do various things deterministically.

Any scripts you write should be entirely self contained. Ask the user which
language they prefer.

The scripts should be callable from the Bash tool.

The script should be stored in the same place as the skill. Add a section to the
SKILL.md on how to use the script. If the script is required to be called, add
that instruction to the <required> block.

# Template Variables

These variables are automatically substituted when skills are installed:
- `/home/clifford/Documents/source/nori/cli/skills` → actual path to skills directory (e.g., `/home/user/.claude/skills`)
- `{{nori_install_dir}}` → actual install directory (e.g., `/home/user`)

Use these in your skill content to create portable paths. This is especially
necessary for making sure scripts are discoverable.
