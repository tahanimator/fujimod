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
| U9 | Locate X-E2 service-mode power-on key combo in firmware | in-progress | U9-worker | Opus | Started: building on U8 findings. |

## Worker notes

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
