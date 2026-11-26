<!--
state: accepted
date: 2026-11-18
replaces: none
code: crates/xe2-capture/src/facts.rs, crates/xe2-capture/facts/xe2-411-service-facts.x70-assumed.json
-->

# 0004 — Borrowed X70 service values only behind an explicit opt-in flag

Status: accepted (2026-11-18)

## Context

Decision 0003 keeps `read_gate`, `reset_guard`, and `slow_read_bytes` null for
X-E2 4.11 until they are located, so capture refuses. The owner still needed a
real-camera state dump. The X70 1.10 values for the three facts (offset `0xf7`=1,
`0xa3`=1, 256 bytes) are a plausible starting point but not X-E2 evidence
(board unit U15).

## Decision

The X70 values may be used only through the explicit `--assume-x70-values` flag,
backed by a separate inputs file
`crates/xe2-capture/facts/xe2-411-service-facts.x70-assumed.json`. The flag
injects the three values into a live `ServiceFacts` (`AssumedX70Values::apply_to`,
`crates/xe2-capture/src/facts.rs`), each marked `UNVERIFIED: borrowed from
x70-110`. The honest facts file is never read-modify-written and keeps all three
null.

## Why

- It unblocks the one owner-approved capture (U16) without pretending the values
  are confirmed X-E2 facts — the manifest records `assumed_x70_values` with
  provenance, and a loud warning precedes any action.
- The override touches only the three gate/guard/size facts: identity
  (04cb:ff80 / X-E2 / 3.05), the 15 stock-code hash checks, the write allowlist,
  the reader hash pin, twice-read/verify, and cleanup all run unchanged.
- Keeping the honest file null preserves the search target (decision 0005) and
  lets the flag be retired once the facts are located.

## Consequences

- Without the flag, capture still refuses (decision 0003).
- `--flash` still refuses regardless of the flag: `flash_reader` stays null.
- Retiring the flag is the signal that the three facts reached high confidence.
