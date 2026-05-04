# Agent Maintenance

This document describes the local long-running maintainer workflow for human maintainers, Codex, Claude Code, and Cortex-based agents. It is public process documentation only. Do not commit the private `meta/` directory.

## Local Meta Files

Maintainers may keep local cross-session context under:

```text
meta/PROJECT_META.md
meta/status.md
```

Rules:

- Read `meta/PROJECT_META.md` and `meta/status.md` before making changes.
- Update `meta/status.md` before ending the session.
- Keep `meta/` ignored by git.
- Do not place `status.md` in the repository root.
- Do not claim unimplemented capabilities as implemented.
- Do not describe policy/risk gates as sandbox containment.

If a local template is needed, start from [Status Template](templates/status-template.md) and copy it into `meta/status.md` locally.

## Required Session Record

Every maintenance session should record:

- files changed;
- commands and tests run;
- decisions made;
- risks and blockers;
- next actions;
- whether full workspace tests, clippy, release gates, or only focused checks were run.

For runtime or capability changes, include the relevant safety boundary: journal/replay, ownership, memory, RAG evidence separation, tool effects, plugin governance, and protected runtime root.

## Git Boundary

`meta/` is local operational context. It should not be staged or committed. If `git status --ignored meta` does not show `meta/` as ignored, add `/meta/` to local exclude or `.gitignore` deliberately before continuing.
