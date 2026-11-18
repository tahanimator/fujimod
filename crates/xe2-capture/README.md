# fujimod-xe2-capture

A local command-line tool for the X-E2 on firmware 4.11 in service mode. It
identifies the camera, checks that its code is stock, and dumps camera state
(DRAM and the 16 KiB boot ROM at `0x80000000`) to a folder on your PC, with a
manifest of identity, addresses, sizes and SHA-256 hashes. With `--flash` it
also backs up the whole internal flash (128 MiB raw NAND) using temporary
reader code that runs in camera RAM and is cleaned up afterwards, the same
mechanism Dev Tools uses on the X70. Nothing goes over the network, and
nothing is stored on the camera.

## Current status: the capture refuses until three facts are located

`plan` and `dry-run` work now. `capture` is refused, and says why, because
three X-E2 4.11 facts are not yet located with high confidence (see
`facts/xe2-411-service-facts.json`, which records the evidence for each fact):

- `read_gate`: the configuration-RAM byte that enables debug memory reads.
- `reset_guard`: the configuration-RAM byte that stops a service-mode shutdown
  from restoring factory defaults.
- `slow_read_bytes`: the largest debug memory read per command.

The **whole-flash backup** (owner-approved) needs those three facts and one
more input: a reviewed reader image, pinned by hash in the facts file
(`flash_reader`). The repo has the host side of the reader path
(`fujimod_workflows::flash`, transport `exr2-query-heap`) but no X-E2 reader
image, so the flash step is refused today. The reused path reads the RAM gates
at offsets `0xf7` (must be 1) and `0xa3`, so the located `read_gate` and
`reset_guard` must agree with those offsets before the reader may run.

When the three facts are added to the facts file with high confidence and
evidence, `capture` runs with no code change. `capture --flash` also needs
the pinned reader (see "Pinning the reader" below).

## Running with assumed X70 values (`--assume-x70-values`)

The X70 and the X-E2 share the EXR II processor, so the owner may choose to try
the X70 1.10 values for the three unlocated facts on the X-E2. The flag
`--assume-x70-values` does exactly that, and nothing more:

- It injects, for this run only, the X70 1.10 values for `read_gate`
  (configuration-RAM offset `0xf7` = 1), `reset_guard` (offset `0xa3` = 1) and
  `slow_read_bytes` (256). They come from the inputs file
  `facts/xe2-411-service-facts.x70-assumed.json`, which records where each one
  was taken from in the repo and marks all three "UNVERIFIED: borrowed from
  x70-110".
- The honest facts file `facts/xe2-411-service-facts.json` is **not** changed:
  those three facts stay `null` / unverified. The override is a separate inputs
  file and a flag, never a fact.
- It supplies **only** those three numeric values. Every other safety gate is
  untouched: the identity checks (`04cb:ff80`, product `X-E2`, internal version
  `3.05`) and the stock-code hash checks still run before any write; the write
  allowlist, the read-only reader hash pin, the twice-read/verify and the
  cleanup checks are all unchanged.
- Before doing anything, the tool prints a loud multi-line WARNING: the values
  are unverified and borrowed from the X70; a wrong RAM gate or guard address
  on the real X-E2 can crash, hang or reset the camera; pull the battery to
  recover; and proceeding is the owner's informed choice.
- Any dump captured this way is labelled in `manifest.json`
  (`assumed_x70_values`) with the three facts and their provenance, so it is
  never mistaken for a capture under confirmed X-E2 facts.

**A dry run cannot confirm the values.** `dry-run --assume-x70-values` sends
nothing to the camera, so it cannot tell whether the borrowed addresses are
correct for the X-E2 — only the real camera reveals that. Treat the first real
`capture --assume-x70-values` as the actual test of these values, and be ready
to pull the battery.

**`--flash` stays blocked.** The override supplies no reader image, so
`capture DIR --flash --assume-x70-values` still refuses the flash backup and
names `flash_reader`. The whole-flash backup needs a reviewed reader pinned by
hash (see "Pinning the reader"), which is a separate step.

## The flash backup

The X-E2's flash is raw NAND with no memory-mapped window, so it can only be
read by code running on the camera. The tool adds no camera code. It hands the
existing EXR II reader path a reviewed reader and adds checks around it:

1. **Before any USB command:** every fact above, the pinned reader, and the
   catalog flash contract must be present. The reader's artifact and image
   must match their pinned SHA-256, pass the payload's own validation against
   the catalog, and pass a static screen (`src/reader_scan.rs`) that refuses
   direct calls into firmware, supervisor and coprocessor instructions,
   processor-state changes and NAND or DMA controller addresses. The catalog's
   stock entries must all be read-side. `build.rs` runs the same hash and
   screen checks at build time and fails the build on a mismatch.
2. **Before the upload:** identity again, both RAM gates already enabled by
   the state capture (the flash step never writes configuration), the stock
   code checks again, and the stock diagnostic callback in place.
3. **Upload and run (RAM only):** a 32-byte bootstrap stub goes into a
   borrowed slot (saved first); the reader goes into a heap block from the
   stock allocator; the reader is published in the diagnostic-query callback
   and reads pages through the stock corrected NAND page reader.
4. **Cleanup:** the callback, bootstrap slot and bulk source are restored and
   the block is freed by the stock release routine. The tool then reads them
   back and compares them with their values before the upload, and stops with
   a report naming anything left behind.
5. **Twice:** the whole flash is read two times, and both reads must match
   chunk by chunk and in total. The SHA-256 is in `manifest.json`, and the CLI
   re-hashes `flash.bin` as written.

A separate command policy covers the flash phase: memory writes only in DRAM,
one byte per command, never over any pinned stock code; diagnostic queries;
reads. It refuses every configuration write, stored write, data upload (E0),
firmware update, restart, console and event command. The freed heap block may
still hold the reader bytes until the camera reuses it; that is RAM and clears
when the battery comes out.

### Pinning the reader

Build the artifact from reviewed reader source with
`cargo run -p fujimod-workflows --features artifact-build --example
flash_reader_build -- RECIPE.json`, check it with the `flash_reader_plan`
example, copy `flash-reader.json` and `flash-reader.bin` into `facts/`, and
set `flash_reader.value` to `{mechanism: "exr2-query-heap", artifact,
artifact_sha256, image, image_sha256, image_bytes, review}` with confidence
`high`. Then rebuild.

## What the tool will and will not do

It sends only these commands. A built-in policy checks every one, and the CLI
audits the USB trace after each run:

- device information (product, internal version, serial);
- configuration-RAM geometry and single-byte reads;
- debug memory reads;
- the stock diagnostic query that points bulk reads at DRAM, then bulk (E1)
  reads;
- with `--flash` only, the flash phase described above.

Outside the flash phase, the only writes are two single bytes in
configuration RAM: the read gate and the reset guard. Each original value is read first and recorded in the
manifest, and both bytes clear when the battery comes out. The tool:

- refuses to go on if USB is not `04cb:ff80`, the product is not `X-E2`, or
  the internal version is not `3.05`. All three are checked before any write;
- runs the stock code checks before any write when the camera allows it. If
  the camera needs the read gate for those reads, the gate is the only byte
  written first, and it is put back if the code is not stock;
- arms the reset guard only after the code checks pass;
- never erases, programs or writes flash, writes stored settings, saves debug
  mode, restarts the camera or updates firmware; it writes RAM and runs code
  only in the `--flash` phase, as described above;
- reads the boot ROM twice and requires both reads to match.

## Windows, step by step

### One-time setup

1. Install Rust from <https://rustup.rs> (the default MSVC toolchain), and Git.
2. Get the code and build the tool:

   ```
   git clone https://github.com/tahanimator/fujimod
   cd fujimod
   cargo build --release -p fujimod-xe2-capture
   ```

   The program is `target\release\fujimod-xe2-capture.exe`. It uses the
   `nusb` USB library, which talks to WinUSB directly, so there is nothing else
   to install.
3. Offline check, with no camera needed:

   ```
   target\release\fujimod-xe2-capture.exe plan
   ```

### Each session

1. Switch the camera OFF. Take the battery out, then put it back in.
2. Connect the USB cable from the camera to the PC.
3. Hold the selector **UP** and press the **shutter fully**, switch the camera
   ON, and keep holding for a few seconds. The rear LED turns solid green, and
   Windows sees a device `USB\VID_04CB&PID_FF80`.
4. First time only: in **Zadig** (<https://zadig.akeo.ie>), choose Options >
   List All Devices, pick the `04CB FF80` device, choose **WinUSB** and click
   Install Driver. If Dev Tools already connects, this is done.
   **Session close.** After a successful `dry-run` or `capture`, the tool
   sends the stock session close (C4) once and prints the camera's two-byte
   reply. A capture also records it in `manifest.json` as `session_end`. You
   can then run again without a battery pull. On the owner's X-E2, Dev Tools
   closed and reopened a session this way with settings intact, but one
   observation is all the evidence there is. The close is never sent after a
   failed run. If a run fails, or the close isn't confirmed, the next run stops
   at once with "the camera already has an open service session", before
   sending any command. Then pull the battery and re-enter service mode
   (steps 1-3).
5. **Run the dry run first.** It identifies the camera, runs the stock code
   checks if the camera allows reads, prints the exact command plan
   (including, for the flash step, the bootstrap address, reader size and
   SHA-256 that would be uploaded), and writes and uploads nothing:

   ```
   target\release\fujimod-xe2-capture.exe dry-run
   ```

   With the honest facts the plan shows the state dump `[REFUSED]` naming the
   three unlocated facts. To try the X70 1.10 values, add the flag — the tool
   prints the WARNING first, then the plan with the state dump `[ready]`:

   ```
   target\release\fujimod-xe2-capture.exe dry-run --assume-x70-values
   ```

   Go on only if it shows `X-E2`, internal version `3.05`, and
   `audited ... FF80 commands: all read-only`. Remember a dry run **cannot**
   confirm the assumed values are right for the X-E2; the first real capture is
   the actual test.
6. Capture into a new folder. With the assumed X70 values, read the WARNING,
   then run:

   ```
   target\release\fujimod-xe2-capture.exe capture xe2-capture-2026-11-18 --assume-x70-values
   ```

   (Without `--assume-x70-values` this refuses until the three facts are located
   on the X-E2.) The folder gets `dram.bin`, `boot-rom.bin` and `manifest.json`;
   when the override was used, `manifest.json` lists `assumed_x70_values` so the
   dump is clearly labelled as captured under unverified parameters. The DRAM
   dump is 512 MiB, so keep the PC awake and the cable still. If the boot ROM
   reads as all zero or all `0xff`, it is masked, not saved. **This first real
   run is the test of the borrowed values: if the camera stops responding or
   reboots, pull the battery (step 8).**
7. To back up the flash as well, add `--flash` to the same command, with a new
   folder:

   ```
   target\release\fujimod-xe2-capture.exe capture xe2-capture-2026-11-18 --flash --assume-x70-values
   ```

   This is still **blocked** today: `--assume-x70-values` does not supply a
   reader image, so the flash step refuses and names `flash_reader`. The
   whole-flash backup needs a reviewed reader pinned by hash (see "Pinning the
   reader"), a separate step from the three service values. Once the reader is
   pinned, the flash phase reads the 128 MiB flash twice into `flash.bin` and
   adds a `flash` section to `manifest.json` (SHA-256, reader hashes, bootstrap
   address and cleanup checks). It ends with "upload area restored". If it stops
   with "cleanup incomplete" or "NOT restored", the leftovers are in RAM only:
   pull the battery as below.
8. **Pull the battery to leave service mode.** Never use the OFF switch: a
   normal shutdown in service mode can restore factory defaults. Unplug USB,
   put the battery back, and switch on normally. If a run with
   `--assume-x70-values` made the camera crash, hang or reset, pulling the
   battery is also how you recover it.

## Tests

```
cargo test -p fujimod-xe2-capture
```

The tests drive the real FF80 session code against a mock camera that applies
the write policy to every command. They cover:

- identity mismatches abort before any write;
- missing facts refuse before any command;
- changed stock code aborts, restoring the read gate if it was opened;
- the dry run never writes;
- a full capture writes exactly the two RAM gates;
- every other write opcode is blocked;
- the flash backup uploads, runs, reads twice and cleans up against a mock
  EXR II camera, and the upload area reads back as before;
- missing facts or a missing reader refuse the flash step before any USB
  command;
- a modified reader image or artifact is rejected, and images with a possible
  flash write path are rejected by the static screen even when pinned;
- the flash-phase policy blocks every flash, firmware, configuration and
  stored write, and a failed cleanup is reported with what was left;
- the honest facts file keeps the three values unlocated and capture refuses
  without the flag; with `--assume-x70-values` the override injects only those
  three facts (identity, code checks and the flash reader unchanged), the state
  dump runs and the manifest records the assumed provenance; identity and
  stock-code mismatches still abort under the override; the write allowlist
  still blocks every non-gate write; and `--flash` still refuses naming the
  reader.
