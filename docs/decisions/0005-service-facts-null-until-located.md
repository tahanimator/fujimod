<!--
state: accepted
date: 2026-11-18
replaces: none
code: crates/xe2-capture/facts/xe2-411-service-facts.json
-->

# 0005 — Service facts stay null until located, never guessed or borrowed

Status: accepted (2026-11-18)

## Context

`xe2-411-service-facts.json` records each service-capture fact with a value, a
confidence, and evidence. For the three gate/guard/size facts, the X70 family
offsets (`0xf7`, `0xa3`, 256) are tempting, and a single `ldrb …,#0xf7` at
`0x126d5f0` looks like the gate — but neither was confirmed from X-E2 4.11 code
(board units U10, U11).

## Decision

A fact is used only when its value is present and its confidence is `high`. A
fact that has not been located in X-E2 firmware (or otherwise confirmed to the
"X-E2 code directly shows the value" bar) stays `null` with confidence
`unlocated`, and the tool refuses every step that needs it, naming the fact. The
honest facts file never holds a guessed or borrowed value.

## Why

- Writing an unverified gate/guard byte to a real camera can crash, hang, or
  reset it; refusing is the safe default.
- A borrowed value that happens to work (decision 0004) is still not evidence;
  conflating the two would lose the distinction the manifest and the facts file
  exist to keep.
- A null fact is a precise search target for later location work (U11/U14).

## Consequences

- Borrowing is possible only through the explicit override of decision 0004.
- Promoting a fact to `high` requires a firmware or emulator trace; doing so
  retires the override and should be recorded as its own decision.
