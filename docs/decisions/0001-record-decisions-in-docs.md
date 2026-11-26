<!--
state: accepted
date: 2026-11-25
replaces: none
code: scripts/docs/decisions.py, mise.toml
-->

# 0001 — Record project decisions in docs/decisions

Status: accepted (2026-11-25)

## Context

FujiMod is reverse-engineering work carried out by many short-lived workers (see
`BOARD.md`), where the reasoning behind a choice is easily lost once the commit
that made it scrolls out of view. Questions such as "why does capture refuse
here" or "why is this file `.generated`" were being re-derived from scratch.

## Decision

Keep dated decision records under `docs/decisions`, one Markdown file per
decision (`NNNN-slug.md`), with a generated index and a validating checker. A
decision is recorded once it is agreed, and superseded rather than rewritten.

## Why

- A numbered, append-only log preserves the order and context of decisions the
  way commit messages and `BOARD.md` rows do not.
- Generating the index and validating cross-references in CI keeps the log
  honest: a dangling `replaces`, a missing `code` path, or a stale index fails
  the build, so the records cannot quietly drift from the code.
- It matches how other decomp/RE projects keep architecture decision records.

## Consequences

- New decisions cost a short file and a regeneration (`mise run decisions`).
- `index.generated.md` is a generated artifact and is never hand-edited.
- This record system is itself decision 0001.
