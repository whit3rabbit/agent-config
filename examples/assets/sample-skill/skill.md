---
name: sample-skill
description: Use when the user asks to demonstrate a minimal Agent Skill end-to-end.
---

## Goal

Show what an installed skill looks like on disk. The library writes one of
these per skill under the harness's `skills/` root, with the frontmatter
above and the body below.

## Instructions

1. Read this file's frontmatter to recognise the skill name and the
   `description` the harness uses for activation.
2. Treat the body (everything after the closing `---`) as plain markdown
   instructions.
3. The example app's `specs.rs` builds a `SkillSpec` whose body matches
   this file; the file itself is not read at runtime, it is bundled to
   show readers what a skill looks like as files.
