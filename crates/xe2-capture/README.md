# fujimod-xe2-capture

A local command-line tool for the X-E2 on firmware 4.11 in service mode. It
identifies the camera, checks that its code is stock, and dumps camera state
(DRAM and the 16 KiB boot ROM at `0x80000000`) to a folder on your PC, with a
manifest of identity, addresses, sizes and SHA-256 hashes. It covers the
read-only half of Dev Tools steps 1 and 2. Nothing goes over the network, and
nothing is stored on the camera.

## Current status: the capture refuses until three facts are located

`plan` and `dry-run` work now. `capture` is refused, and says why, because
three X-E2 4.11 facts are not yet located with high confidence (see
`facts/xe2-411-service-facts.json`, which records the evidence for each fact):

- `read_gate`: the configuration-RAM byte that enables debug memory reads.
- `reset_guard`: the configuration-RAM byte that stops a service-mode shutdown
  from restoring factory defaults.
- `slow_read_bytes`: the largest debug memory read per command.

The **whole-flash backup** is refused by policy, not only for missing facts.
The X-E2's internal flash is 128 MiB of raw NAND with no memory-mapped window.
The only located reader (`exr2-query-heap`, used by Dev Tools) uploads and runs
code on the camera, and this tool never does that.

When the three facts are added to the facts file with high confidence and
evidence, `capture` runs with no code change.

## What the tool will and will not do

It sends only these commands. A built-in policy checks every one, and the CLI
audits the USB trace after each run:

- device information (product, internal version, serial);
- configuration-RAM geometry and single-byte reads;
- debug memory reads;
- the stock diagnostic query that points bulk reads at DRAM, then bulk (E1)
  reads.

The only writes are two single bytes in configuration RAM: the read gate and
the reset guard. Each original value is read first and recorded in the
manifest, and both bytes clear when the battery comes out. The tool:

- refuses to go on if USB is not `04cb:ff80`, the product is not `X-E2`, or
  the internal version is not `3.05`. All three are checked before any write;
- runs the stock code checks before any write when the camera allows it. If
  the camera needs the read gate for those reads, the gate is the only byte
  written first, and it is put back if the code is not stock;
- arms the reset guard only after the code checks pass;
- never erases, programs or writes flash, writes stored settings, saves debug
  mode, restarts the camera, updates firmware, uploads data or writes memory;
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
5. **Run the dry run first.** It identifies the camera, runs the stock code
   checks if the camera allows reads, prints the exact command plan, and
   writes nothing:

   ```
   target\release\fujimod-xe2-capture.exe dry-run
   ```

   Go on only if it shows `X-E2`, internal version `3.05`, and
   `audited ... FF80 commands: all read-only`.
6. Capture into a new folder (once the facts above are located):

   ```
   target\release\fujimod-xe2-capture.exe capture xe2-capture-2026-11-18
   ```

   The folder gets `dram.bin`, `boot-rom.bin` and `manifest.json`. The DRAM
   dump is 512 MiB, so keep the PC awake and the cable still. If the boot ROM
   reads as all zero or all `0xff`, it is masked, not saved.
7. **Pull the battery to leave service mode.** Never use the OFF switch: a
   normal shutdown in service mode can restore factory defaults. Unplug USB,
   put the battery back, and switch on normally.

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
- every other write opcode is blocked.
