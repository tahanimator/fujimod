<!--
state: accepted
date: 2026-11-18
replaces: none
code: crates/xe2-capture/src/policy.rs, crates/xe2-capture/src/facts.rs, crates/xe2-capture/facts/xe2-411-service-facts.json
-->

# 0003 — xe2-capture refuses state capture until the service facts are located

Status: accepted (2026-11-18)

## Context

`fujimod-xe2-capture` dumps an X-E2 (firmware 4.11) over its FF80 service
protocol. Three facts gate a safe read: `read_gate`, `reset_guard`, and
`slow_read_bytes`. On X-E2 4.11 these were never confirmed from firmware — the
X70 1.10 values (`0xf7`, `0xa3`, 256) were a plausible guess, not evidence
(board units U10, U11, U13, U14).

## Decision

Capture is refused until the three facts are located, with `null` recorded in
`xe2-411-service-facts.json`. The X70 values may be used only through an explicit
opt-in flag (`--assume-x70-values`) backed by a separate
`*.x70-assumed.json` inputs file; each borrowed value is marked
`UNVERIFIED: borrowed from x70-110`, and the run prints a loud warning and
records the provenance in its manifest. The honest facts file is never edited to
hold a guess.

## Why

- Writing an unverified gate byte to a real camera risks the owner's device; the
  policy refuses rather than guess (U10).
- The opt-in override keeps the one verified real-camera run (U16, owner's own
  X-E2, owner-approved) reproducible without pretending the values are confirmed.
- Keeping the honest facts file null preserves the search target for U11/U14.

## Testing

Without the flag, `plan`/`dry-run`/`capture` refuse steps that name the facts
(service_capture workflow 11/11). With the flag, the state dump runs after the
warning; identity, stock-code hash checks, the write allowlist, the reader hash
pin, twice-read/verify, and cleanup are unchanged.

## Open

- `read_gate`, `reset_guard`, and `slow_read_bytes` remain unlocated for
  X-E2 4.11; locating them retires `--assume-x70-values`.
- Whole-flash backup (`--flash`) stays refused: no reviewed X-E2 reader image is
  pinned in the repository (U13).
