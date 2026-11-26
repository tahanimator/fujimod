<!--
state: accepted
date: 2026-11-25
replaces: none
code: none
-->

# 0006 — Software-only scope; a physical camera only with explicit owner approval

Status: accepted (2026-11-25)

## Context

FujiMod's X-E2 4.11 work is carried out by many short-lived workers against
firmware files and the emulator (`BOARD.md`). A physical camera can be bricked by
a wrong gate/guard write, and only the owner's own camera may be put at risk.

## Decision

Work is software-only by default: firmware analysis, catalog facts, and the
emulator. A physical camera is touched only with explicit owner approval, on the
owner's own device, and the firmware `.DAT`, captured ROM, and any DRAM dump are
never committed. Only board unit U16 has touched a real camera, under owner
approval.

## Why

- It bounds the blast radius of a mistake to files and the emulator, not
  hardware.
- It keeps the owner in control of the one irreversible risk (a real-camera
  write), consistent with the capture tool's refuse-by-default posture
  (decision 0003) and the opt-in override (decision 0004).

## Consequences

- Capture, flash backup, and emulator bring-up that need a real device or its ROM
  wait for an owner-approved session on the owner's machine.
- Captured artifacts stay local; records cite their hashes, not their bytes.
