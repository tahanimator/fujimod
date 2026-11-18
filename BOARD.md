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
| U10 | Local read-only flash backup + state dumper for X-E2 4.11 (service mode) | review | U10-worker | Opus | Tool `crates/xe2-capture` (`fujimod-xe2-capture plan / dry-run / capture DIR`, Rust on nusb + WinUSB). Identity, dry run, policy and audit work now. State capture is refused until `read_gate`, `reset_guard` and `slow_read_bytes` are located (unlocated in `crates/xe2-capture/facts/xe2-411-service-facts.json`). Whole-flash backup is refused by policy: the only X-E2 NAND reader uploads and executes code. See "U10 findings". |
| U11 | Locate xe2-411 service facts read_gate, reset_guard, slow_read_bytes | blocked | U11-worker | Opus | All three remain UNLOCATED: no X-E2 4.11 code directly shows the values and X70's 0xf7/0xa3 were not borrowed (guessing). Located the FF80 command infrastructure (dispatch table 0x146c024, descriptors 04cb:ff80, session struct 0x15434f0, parser 0x10c89c0) but did not isolate the 200001 read-handler gate/size or the teardown guard-byte to a high bar. Tool unchanged; gates intact; capture still correctly refused; tests 12/12 + workflows service_capture 11/11 pass. See "U11 findings". |
| U13 | Flash backup via temporary RAM code in xe2-capture (owner-approved) | review | U13-worker | Opus | `fujimod-xe2-capture capture DIR --flash` (commit 95caadf) reuses `fujimod_workflows::flash::backup` (EXR II `exr2-query-heap`) unchanged: upload to a stock heap block in RAM, run, read NAND via the stock page reader, clean up; read twice, hashed, cleanup read back and reported. Still REFUSED before any USB command: read_gate/reset_guard/slow_read_bytes unlocated, AND no reviewed X-E2 reader image exists in the repo to pin (the reader source and staged flash-reader artifacts are external). Tests 20/20; workflows 87/87. See "U13 findings". |
| U14 | Emulator trace of xe2-411 service facts (read_gate, reset_guard, slow_read_bytes) | blocked | U14-worker | Opus | Could NOT bring up X-E2 4.11 in the repo's emulator on this machine, so no FF80 handler behavior was observed; all three facts stay null/unlocated (not guessed, X70's 0xf7/0xa3 not borrowed). Blockers: (A) native boot refuses xe2-411 — run.json has null inputs/scene/execution ("configuration xe2-411 declares no startup inputs", reproduced) and open_exr2 requires the 16 KiB boot ROM sha256 4ecedfbf… which is absent everywhere; (B) web/wasm simulated camera needs a prebuilt USB wasm bundle + emulate.usb catalog facts + xe2 boot.service controls + wasm/nightly toolchain, all absent/unavailable; (C) no in-repo fixture runs the firmware answering FF80 (usb-app80 tests use a scripted transport, not an emulated board). Unicorn CPU + emulator crates build/run here — the blocker is firmware bring-up, not execution. Facts evidence updated with the emulator attempt; gates intact; capture still correctly refused. Tests 20/20, service_capture 11/11, plan refuses steps 2-6 naming the facts. See "U14 findings". |
| U15 | Explicit unverified X70-assumed service-values override for xe2-capture | review | U15-worker | Opus | Added an opt-in override: flag `--assume-x70-values` + separate inputs file `facts/xe2-411-service-facts.x70-assumed.json` injects the X70 1.10 values (read_gate offset 0xf7=1, reset_guard 0xa3=1, slow_read_bytes 256) for the three unlocated facts ONLY, each marked "UNVERIFIED: borrowed from x70-110". Honest `xe2-411-service-facts.json` untouched (three facts stay null/unverified). Without the flag capture still refuses; with it the state dump runs after a loud multi-line WARNING. Identity (04cb:ff80 / X-E2 / 3.05), stock-code hash checks, the write allowlist, the reader hash pin, twice-read/verify and cleanup are all unchanged; the manifest records `assumed_x70_values` with provenance. `--flash` still blocked (no reviewed reader pinned). Dry-run cannot confirm the values — first real run is the test. Tests 27/27; workflows service_capture 11/11; clippy clean. See "U15 findings". |

## Worker notes

### U15 findings — explicit unverified X70-assumed service-values override

**Outcome: implemented and tested.** Tom can now run the X-E2 capture using the
X70 1.10 values for the three unlocated service facts, behind an explicit flag,
without those values being recorded as confirmed X-E2 facts and without any
other safety gate being weakened.

**The three borrowed values and where each came from (all X70 1.10):**
- `read_gate` = configuration-RAM offset `0xf7` (247), enabled value `1`.
  Source: the X70 ServiceSetup fixture `debug_offset: 247` in
  `crates/workflows/src/service_mode/tests.rs` (identity.products=["X70"]);
  consistent with `crates/workflows/src/flash/exr2.rs` `configuration_flags`
  reading offset `0xf7` and `require_debug_access` requiring it `== 1`.
- `reset_guard` = offset `0xa3` (163), value `1`. Source: the same X70 fixture
  `guard {offset:163, value:1}`; consistent with `exr2.rs` reading offset
  `0xa3`; `catalog-workflows` requires `guard.value == 1`.
- `slow_read_bytes` = `256`. Source: the same X70 fixture
  `configuration.memory_read_bytes: 256`; `crates/catalog-workflows/src/lib.rs`
  caps `memory_read_bytes` at 256. (The X70 `service-workflow.generated.json`
  is not checked in for any body — U8/U10/U11 — so this in-repo X70 fixture is
  the authoritative in-repo trace of these values.)

**How the override works.** A separate inputs file
`facts/xe2-411-service-facts.x70-assumed.json` carries only those three values,
each with provenance "UNVERIFIED: borrowed from x70-110" and an `x70_source`
pointer. The flag `--assume-x70-values` loads it (`AssumedX70Values::apply_to`)
and injects the three into a live `ServiceFacts` as high-confidence facts whose
evidence begins with the assumed mark. The checked-in honest facts file is
never read-modified-written; it still has all three `null` / `unlocated`.

**What the flag does NOT bypass (reviewed in code and tests):**
- `apply_to` touches only `read_gate`, `reset_guard`, `slow_read_bytes`. It does
  not alter `service_usb`, `product`, `internal_version`, `code_checks` or
  `flash_reader`. So identity (04cb:ff80 / X-E2 / 3.05) and the stock-code hash
  checks run unchanged before any write.
- The write allowlist is still `Policy{read_gate, reset_guard}`: single-byte,
  page-0 RAM writes at exactly those two offsets, enabled-or-restore for the
  gate and enabled-only for the guard. Every other write (other RAM bytes,
  stored config, memory writes, firmware/restart/console/event) is blocked.
- `flash_reader` stays `null`, so `capture --flash --assume-x70-values` still
  refuses before any USB command and names `flash_reader`. The reader hash pin,
  static screen, twice-read/verify and cleanup checks are untouched.
- Before doing anything the tool prints a loud multi-line WARNING (values
  unverified/borrowed from the X70; a wrong gate/guard address can crash, hang
  or reset the camera; pull the battery to recover; owner's informed choice).
- The manifest records `assumed_x70_values` (the three facts + provenance), so
  any dump is labelled as captured under unverified parameters. The dry run
  prints the warning and a note that it CANNOT confirm the values — only the
  real camera can, so the first real run is the test.

**What still blocks the flash backup.** Unchanged from U13: no reviewed X-E2
EXR II reader image is pinned in the repo (`flash_reader` unlocated). The
override deliberately does not supply one. Pinning a reviewed reader (build it
with the `flash_reader_build` example, drop `flash-reader.json/.bin` beside the
facts, set `flash_reader` high) is the separate step; `build.rs` then verifies
hashes + static screen.

**Checks.** `cargo test -p fujimod-xe2-capture` 27/27 (20 prior + 7 new:
honest-facts-stay-unlocated-and-refuse; override-injects-only-the-three;
capture-labels-manifest; override-does-not-bypass-identity/code;
override-policy-blocks-non-gate-writes; override-does-not-unblock-flash;
override-inputs-reject-bad-shapes). `cargo test -p fujimod-workflows --features
fujimod-workflows/service-capture service_capture` 11/11. `cargo clippy
--all-targets` clean. CLI verified offline: `plan`/`dry-run`/`capture` refuse
without the flag; with it the state dump is `[ready]`, `--flash` still
`[REFUSED]` naming `flash_reader`; bad flag combinations give the usage error.
Did not touch any physical camera or USB device and did not contact
fujimod.com. Firmware DAT not committed.

### U14 findings — emulator trace of xe2-411 service facts

**Outcome: the dynamic emulator trace could not be run. The X-E2 4.11 firmware
cannot be brought up in this repo's emulator on this machine, so read_gate,
reset_guard and slow_read_bytes remain `null` / `unlocated`. Per the task
("if the X-E2 can't be started that way, document exactly why and stop; don't
build large new emulator features") I stopped and documented. No value was
guessed; the X70-family 0xf7/0xa3 were not borrowed. The tool, its policy and
its safety gates are unchanged and still correctly refuse the capture.**

I mapped every path the repo has for running an X-E2 and each is blocked here:

- **(A) Native Rust boot** (`crates/emulator-boot` `open_exr2` / the
  `fujimod-emulator boot` CLI). Refused at `prepared()` because
  `configurations/emulator/xe2-411/run.json` declares `inputs`, `scene` and
  `execution` all `null` — the native path requires all three (only the
  `*-rec` configs like `x70-110-rec` carry them). Reproduced empirically:
  `fujimod-emulator boot --configuration xe2-411 --update …FWUP0004.DAT` →
  `configuration xe2-411 declares no startup inputs` (same with the
  `xe2-411-browser-usb` overlay). Independently, `open_exr2` verifies a 16 KiB
  boot-ROM file by sha256 `4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be`
  (CLI: "The ROM dump is required"); that ROM exists nowhere on this machine
  (U0 reported the same for the SD-boot stages — it is the shared X70/EXR-II
  boot ROM and is not inside the DAT). The real-boot tests in
  `emulator-boot/src/tests.rs` themselves skip unless `FUJIMOD_TEST_BOOT_ROM`
  and `FUJIMOD_TEST_UPDATE` are set.
- **(B) Web/wasm simulated camera** (`apps/web/src/dev-tools/simulated-camera.ts`,
  `WasmEngine` + `Exr2UsbBus`). Needs a prebuilt per-firmware USB wasm bundle
  (`firmware.emulate.usb.bundle` + `wasm.json` manifest with a `usb.kind=exr2`
  node); none is on the machine and `catalog/generation.generated.json` has no
  `emulate` facts for any body. It also reads
  `selection.physical_controls.boot.service` for the boot chord, which xe2 does
  not have (U9 deliberately did not add `boot.service` to xe2 controls.json), so
  it throws "no service-mode boot inputs". Building the bundle needs the wasm
  target + pinned nightly, both unavailable (U0: `static.rust-lang.org` blocked).
- **(C) In-process FF80 fixture.** There is no in-repo fixture that runs the
  real firmware and answers FF80: the `usb-app80` / `usb` FF80 tests drive a
  scripted transport (`crates/usb/src/testing.rs` `Script`), i.e. a mock, not an
  emulated board. Invoking the 0x200001 read handler directly on a bare
  `exr2` board (load firmware sections into DRAM, set PC, run under a Unicorn
  mem-read hook) is possible mechanically, but the handler consults a
  configuration-RAM byte and a service-session struct (0x15434f0) that only the
  full boot + USB enumeration + service entry populate; against uninitialised
  state it cannot *demonstrate the gate/bound changing the handler's behaviour*
  (the task's step-4 bar for `high`), and reconstructing that state is exactly
  the "large new emulator feature" the task forbids.

The execution engine itself is fine here: Unicorn (`libunicorn.a`) and the
emulator crates are already built and the board run/trace API
(`board.run_for_instructions`, `read_physical`/`write_physical`,
`read_register`/`write_register`, `board.trace_pcs`, Unicorn mem hooks) works.
The blocker is firmware *bring-up inputs* (boot ROM + declared run inputs /
service-mode controls / a prebuilt USB bundle), not the CPU.

**What would unblock this (for the owner / whoever has the missing inputs):**
either (1) supply the 16 KiB X70/EXR-II boot ROM dump AND author
`configurations/emulator/xe2-411` native run inputs+scene+execution (or a
`service-session` hardware-inputs profile like the existing
`xe2s-service-session` one, which xe2 lacks), then boot and single-step the
0x200001 handler under a mem-read hook to read the gated config offset + enabled
constant + per-read size bound, and the teardown guard offset/value; or (2) run
the external x70view `profile_custom` service-workflow locator for xe2-411; or
(3) build the per-firmware USB wasm bundle and drive it via `simulated-camera`.
Then set each value with `confidence: high` and the before/after reply, and
update the refuse test. No tool or policy change is otherwise needed.

**Checks.** `cargo test -p fujimod-xe2-capture` 20/20; `cargo test -p
fujimod-workflows --features fujimod-workflows/service-capture service_capture`
11/11; `fujimod-xe2-capture plan` shows step 1 identify `[ready]` and steps 2-6
`[REFUSED]` naming the three facts. JSON valid. Did not touch any physical
camera or USB device and did not contact fujimod.com. Firmware DAT not committed.

### U13 findings — flash backup via temporary RAM code (owner-approved)

**Outcome: implemented and tested against a mock camera; still refused for a
real run.** `fujimod-xe2-capture capture DIR --flash` adds a whole-flash backup
to `crates/xe2-capture` (new `src/flash.rs`, `src/reader_scan.rs`, `build.rs`).
No camera code was written: the step hands the existing EXR II reader path,
`fujimod_workflows::flash::backup` with transport `exr2-query-heap` (the X70
Dev Tools mechanism), a reviewed reader pinned by hash, and adds gates around
it. No physical camera/USB device was touched and nothing upstream was contacted.

**What still blocks a real run (two independent blockers):**
1. `read_gate`, `reset_guard`, `slow_read_bytes` are unlocated (U11). Not
   located or guessed here. The flash step also requires the located gates to
   agree with the offsets the reused path reads itself (`exr2.rs`
   `configuration_flags`: debug 0xf7 must be 1, guard 0xa3); otherwise it
   refuses naming the fact.
2. **No reviewed X-E2 reader image exists in this repo.** The repo holds the
   host side (`flash/exr2.rs`) and the xe2-411 flash contract, but
   `build::prepare_exr2` links the image from an externally compiled reader
   (`flash_reader_call`/`flash_reader`/`flash_reader_progress`) plus the corpus
   `exr2_execution` locator and an index. Neither the reader source nor any
   staged `flash-reader.json/.bin` (for any body) is checked in; Dev Tools
   fetches them from the runtime site, which I did not contact. Someone must
   build it from reviewed source (`flash_reader_build` example), drop the
   artifact JSON + image into `crates/xe2-capture/facts/`, and pin
   `flash_reader` (artifact/image sha256, size, review) at confidence high.
   Note: the task's `crates/mod-install` does not exist; the closest is
   `crates/runtime-install`. The exr2 path does its own checked upload and
   ownership-checked cleanup, so no extra crate was needed.

**Enforced in code:**
- Refusal before any USB command unless every service fact, the pinned and
  verified reader, the flash contract and the gate-offset agreement are present.
- Reader verification at build time (`build.rs`: hashes, size, static screen;
  build fails otherwise; only a verified pair is embedded) and at run time
  (hashes, `Payload::validate` against the catalog, static screen, catalog
  stock entries must be read-side: no program/erase/write/format names).
- Static screen (`reader_scan.rs`, every A32 word): refuses direct branches out
  of the uploaded code, BLX-immediate, SVC/SMC, coprocessor (cache/MPU/VFP),
  MSR/CPS/SRS/RFE, and any literal or MOVT in EXR II peripheral space (covers
  the NAND controller 0xfff52000 and DMA 0xfff02000; a test checks every SoC
  block). It is a conservative screen, not a proof; the reviewed hash is the
  primary guarantee, and a legitimate reader could trip it (e.g. VFP code),
  which would refuse rather than run.
- Before the upload: identity again, both RAM gates already enabled (the flash
  phase never writes configuration), stock code checks, stock callback in place.
- `FlashPolicy` for the phase (mock enforces it on the wire, CLI audits the
  trace): RAM writes only in DRAM, 1 byte per command (no write size is located,
  so the minimum is used), never over any pinned stock code; diagnostic queries;
  reads. Configuration, stored, E0, firmware, restart, console and event
  commands are refused.
- Flash read twice; chunks compared by SHA-256 and the totals must match; the
  CLI writes `flash.bin` at chunk offsets and re-hashes the file. Manifest gets
  a `flash` section: size, sha256, chunks, uncorrectable pages, reader hashes,
  bootstrap address, callback slot, per-pass cleanup.
- Cleanup: after each pass the callback slot, bootstrap slot, bulk source state
  and RAM gates are read back and compared with pre-upload values; residue is
  named in the error with the battery-pull instruction. Known residue: the
  freed heap block may still hold reader bytes until reused (RAM only).
- Dry run / plan list the upload (bootstrap address, reader size, image and
  artifact sha256, allocator/release entries, request/buffer offsets); the
  heap address is chosen by the camera at run time, so it is listed as such.
- Battery-pull reminder unchanged on every run.

**Tests:** `cargo test -p fujimod-xe2-capture` 20/20 (8 new: full
upload/run/read-twice/cleanup sequence on the extended mock, missing facts and
missing reader refuse with zero USB commands, modified image/artifact rejected,
unsafe images rejected even when pinned, no flash-write path in the phase
policy or source, cleanup failure reported, dry-run upload listing, screen vs
SoC). `cargo test -p fujimod-workflows --features fujimod-workflows/service-capture`
87/87; `cargo test -p fujimod-usb` passes; clippy clean for the crate. Build
gate checked manually with a temporary pin (bad hash and SVC image both fail
the build), then reverted.

### U11 findings — xe2-411 read_gate / reset_guard / slow_read_bytes

**Outcome: all three left UNLOCATED (confidence `unlocated`, value `null`). No
X-E2 4.11 code directly shows any of the three values, so per the task I did not
guess and did not borrow the X70-family constants (debug offset 0xf7, guard
offset 0xa3 — hard-coded only for the EXR II flash reader in
`crates/workflows/src/flash/{exr2.rs,build.rs}`). The capture tool is unchanged
and still correctly refuses, naming each fact.**

**How the facts are defined/used.** `crates/xe2-capture/src/facts.rs`:
`read_gate`/`reset_guard` = `RamByte{offset, enabled_value}` in configuration
RAM view 1 page 0; `slow_read_bytes` = u32 (tool bound 1..=4096). A fact is used
only when `value` is present AND `confidence == high`; otherwise
`service_capture`/the CLI refuse the dependent step and print the evidence.
These map onto the external **service-workflow** model (`crates/catalog-workflows`
`ServiceSetup`): `read_gate` = `debug_offset`, `reset_guard` =
`guard.offset`/`guard.value` (value must be 1, offset != debug_offset, != 0x80,
< cfg.size), `slow_read_bytes` = `configuration.memory_read_bytes` (<= 256 in
that schema). That document is produced by the owner's private x70view locator
(`profile_custom`) and is **not** checked in for ANY body, so X70 1.10 / X-E2S
1.11 have no in-repo service-workflow either — the only in-repo trace of their
values is the hard-coded 0xf7/0xa3 (gate value 1) in `flash/exr2.rs`
(`configuration_flags` reads `[0xa3,0xf7]`; `require flags[1]==1`). I have only
the X-E2 4.11 DAT here (no X70/X-E2S binaries), so cross-checks are structural.

**X-E2 analysis done (genuine, verified).** Decoded both A32 exec streams from
`/home/claude/fw/FWUP0004.DAT` with `fujimod.firmware.scan`/`decode` (raw DAT,
not inverted): `stream-0016025c` base 0x8a0400 and `stream-0032025c` base
0xf6a9c0, both sha256-matching firmware.json. Disassembled with llvm-mc/an ELF
wrapper for llvm-objdump (ARM/A32, not Thumb — handler pointers are even). Located
the FF80 vendor-command infrastructure in stream-0032025c:
- USB device descriptors `04cb:ff80` at 0x146bfc0 / 0x146bfd4 (confirms U10/HIGH service_usb).
- (command,timeout) table at 0x146c004: 0x550001/0x760001/0x450006 = 1000 ms, 0x660002 = 60000 ms.
- Vendor-command handler-pointer table at 0x146c024: 29 real handlers in
  0x10c9xxx-0x10cfxxx; default/unsupported handler 0x10cf7e0 returns status word 0x2005.
- Request parser 0x10c89c0 copies up to 4 command args into RAM struct 0x154335c.
- Service-session state struct at 0x15434f0 (fields used by the session/mode
  handlers 0x10c99f0/0x10c9b18, result codes 0x2001/0x2003/0x2006/0x201d/0x201e).

**Why still unlocated.** I did not isolate (to "X-E2 code directly shows it"):
(a) the 200001/function-2 (read_memory) handler that consults the config-RAM
gate byte and compares the enabled constant → read_gate; (b) the size-limit
comparison in that same handler → slow_read_bytes; (c) the service-mode teardown
path that reads the guard byte → reset_guard (U9/restart.generated.json put the
`service_command` sender at 0x10F4BA8, sub-senders 0x10F4C28..0x10F5914, but the
shutdown-time guard read was not pinned). Offset-byte search is not decisive:
`ldrb …,#0xa3` occurs at dozens of sites (common struct offset); `ldrb …,#0xf7`
occurs once (s32 0x126d5f0) but is unverified as the gate. The handler-table
index→command-code mapping (two-level; not a direct group-byte index) was not
resolved, so I could not name the 200001 handler with certainty.

**Next steps (for the owner / whoever has x70view).** Either run the external
`profile_custom` service-workflow locator for xe2-411, or finish the trace:
resolve the 0x146c024 dispatcher's code→index map, disassemble the 200001 handler
to read the gated config offset + enabled constant + per-read size bound, and the
teardown for the guard offset/value; cross-check each routine's structure against
X70 1.10 / X-E2S 1.11 (needs those binaries). Then set value + `confidence:high`
with addresses/excerpts in `facts/xe2-411-service-facts.json` and update the
`bundled_facts_refuse_capture…` test. No tool/policy change is otherwise needed.

**Checks.** `cargo test -p fujimod-xe2-capture` 12/12; `cargo test -p
fujimod-workflows --features fujimod-workflows/service-capture service_capture`
11/11; JSON parses; `fujimod-xe2-capture plan` shows step 1 identify ready and
steps 2-6 refused naming the facts. Firmware DAT not committed. Did not touch any
camera/USB device or contact fujimod.com.

### U10 findings — local read-only X-E2 4.11 capture tool

**Tool:** `crates/xe2-capture`, binary `fujimod-xe2-capture`, a Rust CLI on the
existing `fujimod-usb` FF80 session (nusb, which works with the WinUSB driver
Zadig binds) and `fujimod-workflows::state::dump`. Commands: `plan` (offline),
`dry-run` (no writes), `capture DIR`. Windows steps are in
`crates/xe2-capture/README.md`, with a summary in the root README. I picked
Rust over Python because the Python bindings also need a Rust build of the
native extension, and the USB stack already uses nusb.

**What `profile_custom` is:** not in this repo. It is a locator in the owner's
private x70view tooling that feeds `service-workflow.generated.json`
(`crates/catalog-workflows`). Dev Tools' "Back up flash" loads that document
for the `ServiceSetup` facts: `ram_alias`, the configuration geometry,
`debug_offset`, and `guard` (offset, aliases, code checks, state words,
callback table, output state, payloads). It fails before sending a command
because xe2-411 has no such document. A capture needs only a subset of it:
the read gate (`debug_offset`), the reset guard, and the debug read size.
`crates/workflows/src/service_capture.rs` lists the same subset.

**Facts** (hand-authored, with evidence, in
`crates/xe2-capture/facts/xe2-411-service-facts.json`):
- HIGH: service USB `04cb:ff80`. Firmware USB device descriptors at 0x0146bfc0
  and 0x0146bfd4 in stream-0032025c, plus the owner's enumeration.
- HIGH: product `X-E2` and internal version `3.05`, from the catalog bulk and
  execution facts.
- HIGH: stock code checks. All 24 bulk/upload code checks and the mapped probe
  re-hash to their catalog sha256 against the streams decoded from
  FWUP0004.DAT.
- Entry (UP + full shutter, USB connected): from the owner's hardware test,
  not firmware.
- UNLOCATED: `read_gate`, `reset_guard`, `slow_read_bytes`. The X70-family
  values hard-coded in `crates/workflows/src/flash/` (guard 0xa3, debug 0xf7)
  were not confirmed for xe2-411 and are not borrowed. I stopped the firmware
  analysis for these facts before confirming them, so the capture step refuses
  and names them. Whoever supplies them must add evidence and set
  `confidence: high`; no code change is needed.

**Flash backup refused by policy.** xe2-411 flash is 128 MiB raw NAND with no
mapped window. Its only located reader, `exr2-query-heap`, allocates heap,
uploads and executes a reader image, and swaps the stock diagnostic callback.
Those writes and that code execution are outside the two-RAM-gate rule, so
there is no read-only whole-flash path for this body.

**Safety enforcement:**
- `policy.rs` allowlists by FF80 bytes: device info, configuration geometry and
  reads, memory function 2 reads, diagnostic query tag 0x80, E1 reads, and
  session envelopes. Everything else is blocked, including memory function 3,
  stored-view configuration writes, E0 uploads and other RAM bytes. RAM gate
  writes are single bytes at page 0, at the located offsets only, with value
  enabled or restore-to-0 (the guard is never cleared).
- `lib.rs` has a single write call site, which checks its own bytes against the
  policy before sending. The CLI audits the USB trace incrementally, and the
  mock camera in the tests applies the policy to every command.
- The tool refuses non-query bulk sources, which would need memory writes.
- Order:
  1. identity (USB IDs, product, version);
  2. read and record the gate and guard originals;
  3. stock code checks with the gate as found;
  4. only if the camera refuses those reads, enable the read gate, re-check,
     and restore it on a mismatch;
  5. arm the guard;
  6. `state::dump` (the boot ROM is read twice).
  Every run reminds the owner to pull the battery, never use OFF.

**Tests:** `cargo test -p fujimod-xe2-capture` passes 12/12. They cover a mock
FF80 camera on the real session code: identity mismatches,
missing-facts refusal, code mismatch with the gate open and closed, the dry run
never writing, a full capture writing exactly the two gates, the policy
opcode table, and a source scan. `cargo test -p fujimod-usb -p
fujimod-workflows --features fujimod-workflows/service-capture` passes. The
`fujimod-crate-graph` layer test has two failures that are not from this unit
(`fujimod-emulator-board-graph` is unlisted; `fujimod-emulator-boot` depends
on model crates). No Python code changed.

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
