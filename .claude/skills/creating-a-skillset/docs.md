# Noridoc: creating-a-skillset

Path: @/.claude/skills/creating-a-skillset

### Overview

- Interactive wizard skill that decomposes a role or task domain into its constituent processes and encodes each as an individual skill within a new skillset
- Produces a complete profile directory containing skills, CLAUDE.md, and nori.json manifest
- Operates directly in the profiles directory (`/home/amol/.nori/profiles/`) without creating git worktrees

### How it fits into the larger codebase

- Part of the Nori skills system alongside other skills in `@/.claude/skills/`
- Delegates to `@/.claude/skills/creating-skills/SKILL.md` in Step 6 to encode each individual process as a skill -- this is a hard dependency
- Uses `nori-web-search-researcher` subagents (via the Task tool) during Step 4 to research the domain's subtasks and methodologies
- The output of this skill is a new profile directory at `/home/amol/.nori/profiles/<name>/` with the same structure as existing profiles (skills/, subagents/, slashcommands/, CLAUDE.md, nori.json)
- The source copy lives at `@/.nori/profiles/amol/skills/creating-a-skillset/SKILL.md` and is installed to `@/.claude/skills/creating-a-skillset/SKILL.md` during profile activation
- After completion, the user activates the new skillset via `/nori-switch-skillset`

### Core Implementation

- 9-step interactive wizard: gather identity, understand domain, scope check, deep research, present decomposition, encode skills, write CLAUDE.md, create nori.json, summary
- **Step 1 (Identity)** supports cloning an existing skillset or building from scratch; validates name uniqueness against `/home/amol/.nori/profiles/`
- **Step 3 (Scope Check)** enforces a coherence litmus test -- "would the processes naturally reference each other?" -- and rejects domains that are too broad (grab-bag) or too narrow (single skill)
- **Step 4 (Research)** launches parallel `nori-web-search-researcher` subagents to decompose the domain, find frameworks, identify pitfalls, and catalog standard tools/artifacts
- **Step 6 (Encode)** reads and follows `creating-skills/SKILL.md` for each approved process, placing output in `/home/amol/.nori/profiles/<skillset-name>/skills/<skill-name>/SKILL.md`
- Uses `{{skills_dir}}` template variable in generated CLAUDE.md files to maintain portability across installations

### Things to Know

- The `<system-reminder>` block explicitly prevents this skill from creating git worktrees or branches -- it operates directly in the profiles directory
- Each candidate process from Step 4 must pass three criteria: concrete (a person actually does it), repeatable (applies across situations), and self-contained (clear inputs and outputs)
- Step 5 requires user approval of the decomposition before any skills are encoded -- the user has final say over the process list
- The skill generates cross-references between sibling skills when their inputs/outputs are related (e.g., positioning-messaging depending on competitive-analysis outputs)

Created and maintained by Nori.
