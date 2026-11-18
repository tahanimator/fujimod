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
| U0 | Toolchain bootstrap; document how a stage is refreshed from stale to pass | done | U0-worker | Sonnet 5.5 (no subagents) | Toolchain built and shared (see Refresh procedure). No generator/runner for progress or catalog facts exists in this repo, so no stage can be flipped from here; facts files for xe2 4.11 already exist and validate. |
| U1 | Bulk read source located; Flash reader located | todo | | | |
| U2 | State-dump regions located; USB restart request located | todo | | | |
| U3 | Optional card file access located | todo | | | |
| U4 | Physical controls mapped to firmware inputs; Settings layout located | todo | | | |
| U5 | EXR II NAND bootloader facts located; Boot-patch triggers, clears and free spaces located | todo | | | |
| U6 | Emulator: SD boot v2 autostart starts; patch triggers fire; FujiMod boots from SD (needs U5) | todo | | | |
| U7 | Language font bank located and kept out of mod memory | todo | | | |
| U8 | X-E2 4.11 `service` and `first-start` fact tables (missing in catalog/generation.generated.json) | blocked | U8-worker | Sonnet 5.5 (no Opus/Fable subagent could be spawned: no agent tool in this session) | No generator for either table exists in this repo, so neither can be regenerated and generation.generated.json is unchanged. `first-start` = `catalog/firmware/<fw>/first-start.generated.json` (only x70-110, xt20-211 have one); failing locator for xe2-411 is `ui_frame_write_triplet_113d8` (also fails for xe2s, x100t, x30, xt10), which lives in the external x70view runner. `service` = `service-workflow.generated.json`, a runtime artifact staged by apps/web/scripts/stage-site.ts from an external runtime dir, never checked in for any firmware; xe2-411 fails on `profile_custom`. Needs the external runner (or owner-authored generator). Service combo: not confirmed from the X-E2 firmware; `boot.service` NOT added to xe2 controls.json (see final report). Firmware does contain a `service_command` routine (restart.generated.json, address 17779624) so service mode exists, but the button check was not located: main-fw scanner (event table 0xB84638, lookup 0x93EF68) has no external callers for UP/S1/S2 at boot, and the flash loader unit at 0 shows only one GPIO bit test. Other bodies with UP/nav+full-shutter: x70, xe2s, xt2, xt20, xh2s (family convention, not X-E2 evidence). |
| U9 | Locate X-E2 service-mode power-on key combo in firmware | done | U9-worker | Opus | LOW confidence: no power-on button combo confirmed. Main firmware never samples UP(2) or S2(8) as held keys (is-key-held primitive 0x93F008 has 3 callers only: LENS_DET/PLAY/S1), so the x70/xe2s family combo (selector.up + full shutter) has no detection code in xe2-411 — consistent with the owner's failed USB test. `boot.service` NOT added to xe2 controls.json. See "U9 findings" below. |

## Worker notes

### U9 findings — X-E2 service-mode power-on combo

**Confidence: LOW. No power-on button combo is confirmed, and I did NOT edit
`catalog/bodies/xe2/controls.json` (no `boot.service` added).**

What I did: disassembled xe2-411 4.11 (streams decoded with the Python
`fujimod.firmware` bindings; ARM32 via `llvm-mc --disassemble`, the two exec
streams are base 0x8A0400 and 0xF6A9C0).

Evidence (addresses are absolute load addresses):
- `service_command` @ 0x10F4BA8 (17779624) is only a message sender: it builds a
  type-2 request and calls `message_send` (~0x9D0DB4). Its ~15 callers
  (0x10F4C28..0x10F5914) are the in-service-mode sub-command senders reached over
  USB (the FF80 diagnostic session), **not** a boot-time key gate.
- Key state is read through the scanner table at 0xB84638 (66 entries, stride 28;
  lookup 0x93EF68). The "is this key currently held" primitive is **0x93F008**
  (control-ids `input_readers.scanner.routine`; returns the stable bit at
  table-offset 21). It has exactly **three** callers in the whole main firmware,
  and the key codes they query are: `#55` KEY_LENS_DET (@0x9E9330), `#11`
  KEY_PLAY and `#7` KEY_S1 (both @0x1201C60/0x1201C9C, a playback/scene init
  task-list routine registered at 0x11FE5AC..). **UP (event 2) and S2 (event 8,
  full shutter) are never queried as held keys anywhere in the app.**
- So the x70/xe2s family service combo — selector UP + full shutter held at
  power-on — has **no matching detection code** in xe2-411. This agrees with the
  owner's real-camera test: that combo did not bring the X-E2 up over USB.
- Catalog corroboration: `catalog/generation.generated.json` shows xe2-411
  `facts` = available (the USB-side service entry / debug gate / FF80 access were
  located by the external runner) but `service` (the service-*workflow* runtime
  artifact) = unavailable (`profile_custom: ... found 0`), and that artifact is
  never checked in for any body (U8). The combo itself is never firmware-derived:
  on x70/xe2s it is a **hand-authored** `boot.service` entry (human hardware
  knowledge), not output of any locator. There is therefore no in-repo or
  in-firmware source that pins an X-E2 service combo.
- FujiMod's "service mode" (crates/workflows/src/service_mode.rs, debug_mode.rs,
  crates/usb/src/ff80) operates on a camera **already in** a stock USB service
  session and arms a debug-access byte; the key chord is only what puts the camera
  into that session at power-on. Nothing samples UP/S2 to do that on the X-E2.

Electrical map (from control-ids inputs, for reference): selector UP = ADC ch 8
(`controls-adc-8`, window 0-255); shutter S1/full-S2 = GPIO bank0 pin2/pin3
(`controls-gpio-0-2` / `controls-gpio-0-3`); DISP/BACK (firmware-update combo) =
ADC ch 9 window 597-724; command_dial press + fn7 (SD combo) =
`controls-gpio-0-10` + `controls-gpio-0-11`.

What the owner should do:
1. The family combo is **ruled out** — do not expect selector-UP + full-shutter at
   power-on to enter X-E2 service mode (owner already confirmed it does nothing
   over USB, and the firmware has no code that would react to it).
2. No alternative button chord is supported by the firmware, so I cannot offer a
   high/medium candidate to try. Ranked candidates, all **low/speculative**, only
   if the owner wants to experiment (battery removed/reinserted first, hold the
   chord, switch ON, keep holding ~5-10 s, then connect USB):
   a. selector UP + full shutter (family convention) — already tested, failed.
   b. the only keys the firmware polls as held at startup are PLAY and S1, so a
      PLAY-held or PLAY + half-shutter power-on is the nearest firmware-grounded
      guess — but these map to playback/scene init, not a confirmed service gate.
   c. most likely the X-E2 USB service/debug session is not reachable by a cold
      power-on chord at all and needs the stored debug-access byte set via the
      FF80 path (a service-tool step), not buttons.
3. Whatever is tried: **exit service mode only by pulling the battery — never with
   the OFF switch** (the stock teardown would restore defaults / clear state on a
   normal power-off).

### Refresh procedure (U0)

**Finding: the stage runner is not in this repository.** `catalog/progress/xe2/4.11/progress.generated.json`
is a *recorded ledger* written by an external bring-up runner (the x70view research
tooling the upstream docs mention). `crates/catalog/src/progress.rs` only defines and validates the
document ("a result whose inputs changed since it was recorded is `stale`"). Nothing here (mise tasks,
`scripts/`, `crates/catalog*`, `crates/corpus`, `bindings/python`, CI) writes it, and no path in the repo
takes a local `.DAT` for stage grading. The same stages are stale in nearly every firmware's file
(including x70 1.10), so `stale` marks a runner-side input change, not missing facts. Do not hand-edit
`*.generated.*`. Consequences for workers:
- The ten fact files in `catalog/firmware/xe2/4.11/` (`bulk`, `flash`, `state`, `restart`, `card-files`,
  `control-ids`, `settings`, `boot-patch`, `memory-reservations`, `sd-boot-v2-autostart`, plus ui-*) already
  exist, and `catalog/generation.generated.json` reports every xe2-411 table as `available`
  (only `first-start` and `service` are `unavailable`, with reasons recorded there). So U1-U5 are
  verification/regeneration of existing facts, not first-time research; any change needs the missing runner
  or a hand-authored change to the generator inputs, which must be raised to the owner.
- Facts are validated in-repo by `cargo test -p fujimod-catalog -p fujimod-catalog-schemas
  --features fujimod-catalog-schemas/schema` (passes: 20 unit + integration tests, schema check of every
  catalog document incl. progress). Schemas regenerate with
  `cargo run -p fujimod-catalog-schemas --features schema --bin catalog-schemas -- --write` (`--check` verifies).
- Building blocks that do exist for analysis: `fujimod.firmware` (decode/scan/outer_header/overlays) and
  `fujimod.corpus` (Index/Store, locators) in the Python extension; `cargo run -p fujimod-firmware --bin
  fujimod-decompress`; `crates/corpus/examples/capture_facts.rs` (JSON-lines flash/boot-ROM locator driver
  over a corpus store, needs a prebuilt index); `fujimod-emulator boot --configuration ID --update DAT`.
- Emulator stages (`sd-boot-v2-*`) need a real 16 KiB X70 boot ROM dump (sha256 `4ecedfbf...f9be`, per
  firmware.json `boot_rom`; `progress.boot_rom` is `X70`) supplied via `--boot-rom`. None exists on this
  machine, and the CLI refuses without it; also `fujimod-emulator boot` currently supports only the
  `x70-110-*` configs ("xe2-411 declares no startup inputs").

**Toolchain state (shared, reuse as is; build outputs are in the default `target/` and `.venv/`):**
- Rust: pinned `nightly-2026-10-10` cannot be installed (`static.rust-lang.org` is blocked by the egress
  policy, so `rustup toolchain install` fails). Stable rustc 1.97.0 builds and tests the catalog and emulator
  fine; use plain `cargo ... --locked`. `rustfmt`/`clippy` nightly-only checks and wasm builds are unavailable.
- `pnpm install --frozen-lockfile` done (node 22.23 is used instead of pinned 26; pnpm 12.18.0 matches).
- Submodule initialised: `git submodule update --init crates/emulator-cpu-unicorn/vendor/unicorn`
  (GitHub works; needed by the emulator CPU backend).
- Built: `cargo build --locked --release -p fujimod-emulator-cli --bin fujimod-emulator` (about 10 min on
  2 cores, output `target/release/fujimod-emulator`); `cargo build --locked -p fujimod-catalog` (about 1 min).
- Python: `uv python install 3.14` (uv here is 0.11.32 and does not know 3.14.8; 3.14.6 is used),
  `uv sync --locked --python 3.14`, `apt-get update && apt-get install -y libjpeg-dev pkg-config`
  (needed by `fujimod-python-host-image`), then `uv run --locked python scripts/python_extension.py`
  (about 8 min) installs `bindings/python/python/fujimod/_native.abi3.so` (git-ignored). Check with
  `uv run --locked python -c "import fujimod.corpus, fujimod.firmware, fujimod.catalog"`.
- Gotchas: only 2 CPUs and 7 GB RAM, so run one heavy cargo build at a time (concurrent builds only
  contend). mise is not installed; mise tasks in `mise.toml` map directly to the commands shown. Run
  Python that touches the DAT with `uv run --locked python -I`; the DAT is read-only (`/home/claude/fw`).

### Owner camera test (coordinator, 2026-11-18)
Holding selector UP + full shutter while powering on **with USB connected** leaves the rear LED solid green and the X-E2 enumerates on Windows as `USB\VID_04CB&PID_FF80` (the FF80 service device) with no driver bound. Without USB the combo does nothing. So the family combo (x70/xe2s `service`, `usb: true`) appears to hold for the X-E2. U9's "no key sampling in main firmware" fits a boot-ROM/loader-level check. Pending: WinUSB binding so WebUSB/Dev Tools can open the device, then FF80 identification before adding `boot.service` to controls.json.
