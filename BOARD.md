# FujiMod X-E2 4.11 — work board

Fork-local coordination board (tahanimator/fujimod). Each worker claims one
unit, keeps its own row current, and commits this file with only its own row
changed (`python scripts/git/commit.py -m "..." --patch` or the named-path form
from AGENTS.md). Status: `todo` → `claimed` → `in-progress` → `review` →
`done` / `blocked`.

Firmware: X-E2 4.11, `FWUP0004.DAT`, sha256
`d324e1475dd99d5ebace3bf689262df671829adb32262ba13cd9b0743ca99bb7`, at
`/home/claude/fw/FWUP0004.DAT` (never commit it).

Scope: software-only stages in `catalog/progress/xe2/4.11/`. Nothing here
touches a physical camera.

| Unit | Stage(s) in progress file | Status | Owner | Models used | Result / notes |
|---|---|---|---|---|---|
| U0 | Toolchain bootstrap; document how a stage is refreshed from stale to pass | todo | | | |
| U1 | Bulk read source located; Flash reader located | todo | | | |
| U2 | State-dump regions located; USB restart request located | todo | | | |
| U3 | Optional card file access located | todo | | | |
| U4 | Physical controls mapped to firmware inputs; Settings layout located | todo | | | |
| U5 | EXR II NAND bootloader facts located; Boot-patch triggers, clears and free spaces located | todo | | | |
| U6 | Emulator: SD boot v2 autostart starts; patch triggers fire; FujiMod boots from SD (needs U5) | todo | | | |
| U7 | Language font bank located and kept out of mod memory | todo | | | |

## Worker notes

U0 records the refresh procedure here for everyone else.
