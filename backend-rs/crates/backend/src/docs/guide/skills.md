# Skills

A skill is a named block of instructions an agent can load when it needs it, instead of carrying it in the system prompt all the time.

## Fields

- **name** — how the agent refers to the skill.
- **description** — the one line the agent reads when deciding whether to load it. Write it as a trigger: when this skill applies.
- **body_markdown** — the instructions themselves.
- **metadata** — optional structured data.

## How an agent uses one

Skills attached to an agent are mounted for the turn. The agent sees each skill's name and description, and calls `SkillManager` to read the body of the one it wants. Bodies are not injected up front, so mounting several is cheap.

## Importing

- **Raw** — paste markdown with YAML frontmatter.
- **Package** — upload a skill archive.
- **GitHub** — import from a repository URL.

Imported skills may carry resource files alongside the body, browsable from the skill's detail page.

## Notes

- `SkillManager` reads mounted metadata and instructions only. It does not execute skill resources or load files from disk.
- A skill must be attached to an agent to have any effect; creating one alone changes nothing.
