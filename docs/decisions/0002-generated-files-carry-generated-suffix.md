<!--
state: accepted
date: 2026-11-25
replaces: none
code: AGENTS.md, catalog/firmware-versions.generated.json
-->

# 0002 — Checked-in generated files carry a .generated suffix

Status: accepted (2026-11-25)

## Context

The repository checks in many derived artifacts — catalog facts, binding types,
docs data — alongside the hand-authored inputs and generators that produce them.
Without a convention, a reviewer cannot tell at a glance which files are safe to
edit, and a well-meaning hand edit to a derived file is silently overwritten by
the next generation.

## Decision

Every checked-in generated file carries `.generated` before its extension
(`control-ids.generated.json`). Generated files are never edited by hand: change
the inputs or the generator and regenerate. A single file is never part generated
and part hand-authored — a generator writes whole files, and hand-authored facts
live in separate files that generators only read.

## Why

- The suffix makes the generated/authored boundary visible in every path and
  diff, so reviewers and tools can treat the two differently.
- Each generator ships a `*-check` task that regenerates and diffs against the
  checked-in copy, so the check fails whenever the two disagree — only possible
  when a file is wholly generated.
- This decision record follows the rule: its own index is `index.generated.md`.

## Consequences

- Adding a generator means also adding its check and listing it in CI.
- Mixed-content files must be split before they can be generated.

See `AGENTS.md` for the authoritative wording of this policy.
