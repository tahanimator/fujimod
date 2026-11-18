//! Owner-approved whole-flash backup through temporary code in camera RAM.
//!
//! This module adds no camera code. It hands the repo's existing EXR II NAND
//! reader path, `fujimod_workflows::flash::backup` with the `exr2-query-heap`
//! transport (the mechanism Dev Tools uses on the X70), a reviewed reader
//! image whose hashes are pinned in the facts file. That path, unchanged:
//! checks stock code, borrows a 32-byte bootstrap slot, allocates a heap block
//! with the stock allocator, uploads and reads back the reader, publishes it in
//! the stock diagnostic-query callback slot, reads NAND pages through the
//! stock corrected page reader, then restores the callback, frees the block,
//! restores the bootstrap slot and the bulk source, and re-checks the
//! configuration flags. Everything stays in RAM; nothing is stored.
//!
//! Around it this module adds gates and checks:
//! - the step needs every service fact the state capture needs, plus a pinned
//!   reader, and refuses before any USB command without them;
//! - the reader's artifact and image must match their pinned SHA-256, pass the
//!   payload's own validation against the catalog, and pass the static screen
//!   in [`crate::reader_scan`]; the catalog's stock entries must be read-side;
//! - identity, both RAM gates (already set by the capture, never written here)
//!   and stock code are checked before the upload;
//! - the flash is read twice and both passes must agree chunk by chunk;
//! - after each pass the callback slot, bootstrap slot, bulk source state and
//!   RAM gates are read back and compared with their values before the upload.
use crate::{
    Error, Inputs, Result,
    facts::{FLASH_MECHANISM, FlashReader, Missing, UsbIds},
    hex, policy, reader_scan,
};
use fujimod_catalog::capture::FirmwareFlash;
use fujimod_catalog_bulk::{BulkSource, FirmwareBulk};
use fujimod_usb::{
    ff80::{
        Session, SessionState,
        commands::memory::{Address, DebugMemoryLimits, MemoryRange},
    },
    operations::{checks::sha256_bytes, memory::read_range},
};
use fujimod_workflows::flash::{
    Artifact, Callbacks, Payload,
    exr2::{self, Configuration},
};
use serde::Serialize;
use std::{collections::BTreeMap, num::NonZeroU32};

include!(concat!(env!("OUT_DIR"), "/bundled_reader.rs"));

/// The RAM-gate offsets the reused EXR II path reads itself
/// (`exr2::configuration_flags`: guard 0xa3, debug 0xf7, debug must be 1).
/// The located xe2-411 facts must agree before the reader may run; these are
/// a consistency check, never a substitute for the facts.
pub const EXR2_READ_GATE: u32 = 0xf7;
pub const EXR2_RESET_GUARD: u32 = 0xa3;
/// Bootstrap slot the reused path borrows: `configuration + 0x80`, 32 bytes.
const BOOTSTRAP_OFFSET: u64 = 0x80;
const BOOTSTRAP_BYTES: u64 = 32;
/// Request block and NAND buffer offsets inside the reader's heap block.
const REQUEST: u64 = 0x1000;
const BUFFER: u64 = 0x2000;
/// Stock entry names that would mean a write path in the catalog contract.
const WRITE_WORDS: [&str; 4] = ["program", "erase", "write", "format"];

/// A reader that passed every pin and screen. Only [`Reader::verify`] makes one.
#[derive(Clone, Debug)]
pub struct Reader {
    pin: FlashReader,
    payload: Payload,
    image: Vec<u8>,
}

fn sha(bytes: &[u8]) -> String {
    hex(&sha256_bytes(bytes))
}
fn refuse(message: impl Into<String>) -> Error {
    Error::Refused(message.into())
}

impl Reader {
    /// The reader embedded at build time, if the facts pin one.
    pub fn bundled(
        inputs_facts: &crate::facts::ServiceFacts,
        flash: &FirmwareFlash,
        bulk: &FirmwareBulk,
    ) -> Result<Option<Self>> {
        let Ok(pin) = inputs_facts.flash_reader.usable("flash_reader") else {
            return Ok(None);
        };
        let Some((artifact, image)) = BUNDLED_READER else {
            return Err(refuse(
                "flash_reader is pinned but no reader was embedded at build time",
            ));
        };
        Self::verify(&pin, artifact, image, inputs_facts, flash, bulk).map(Some)
    }

    /// Check a reader against its pin, the catalog and the static screen.
    pub fn verify(
        pin: &FlashReader,
        artifact: &[u8],
        image: &[u8],
        facts: &crate::facts::ServiceFacts,
        flash: &FirmwareFlash,
        bulk: &FirmwareBulk,
    ) -> Result<Self> {
        pin.validate().map_err(Error::Refused)?;
        if sha(artifact) != pin.artifact_sha256 {
            return Err(refuse(
                "flash reader artifact differs from its pinned sha256",
            ));
        }
        if sha(image) != pin.image_sha256 || image.len() != pin.image_bytes as usize {
            return Err(refuse(
                "flash reader image differs from its pinned sha256 or size",
            ));
        }
        let parsed: Artifact = serde_json::from_slice(artifact)
            .map_err(|e| refuse(format!("flash reader artifact does not parse: {e}")))?;
        if parsed.image.as_deref() != Some(pin.image.as_str()) {
            return Err(refuse("flash reader artifact names a different image"));
        }
        let Payload::Exr2Query(p) = &parsed.payload else {
            return Err(refuse(
                "flash reader is not an EXR II exr2-query-heap payload",
            ));
        };
        if p.firmware_id != facts.firmware_id || p.firmware_sha256 != facts.firmware_sha256 {
            return Err(refuse("flash reader was built for other firmware"));
        }
        if flash.firmware_id != facts.firmware_id
            || flash.transport.as_deref() != Some(FLASH_MECHANISM)
        {
            return Err(refuse(
                "catalog flash contract is not xe2-411 exr2-query-heap",
            ));
        }
        read_side_only(flash)?;
        // The payload's own validation: ABI, sizes, image hash, and that every
        // execution primitive lies inside a pinned stock code check.
        parsed
            .payload
            .validate(flash, bulk, image)
            .map_err(|e| refuse(format!("flash reader payload: {e}")))?;
        reader_scan::scan(image).map_err(|findings| {
            refuse(format!(
                "flash reader image fails the static screen: {}",
                reader_scan::describe(&findings)
            ))
        })?;
        Ok(Self {
            pin: pin.clone(),
            payload: parsed.payload,
            image: image.to_vec(),
        })
    }

    fn exr2(&self) -> &exr2::Payload {
        match &self.payload {
            Payload::Exr2Query(p) => p,
            _ => unreachable!("verified exr2 payload"),
        }
    }

    pub fn pin(&self) -> &FlashReader {
        &self.pin
    }
}

/// The catalog's stock reader contract must name read-side routines only.
pub fn read_side_only(flash: &FirmwareFlash) -> Result<()> {
    let names = flash
        .code_ranges
        .iter()
        .map(|r| r.name.as_str())
        .chain(flash.reader.as_ref().map(|r| r.symbol.as_str()))
        .chain(flash.state_checks.iter().map(|c| c.name.as_str()));
    for name in names {
        let lower = name.to_ascii_lowercase();
        if WRITE_WORDS.iter().any(|w| lower.contains(w)) {
            return Err(refuse(format!(
                "catalog flash contract names a write-side routine: {name}"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct UploadRecord {
    pub mechanism: &'static str,
    pub artifact_sha256: String,
    pub image_sha256: String,
    pub image_bytes: u32,
    pub review: String,
    /// Where the bootstrap stub goes: `configuration + 0x80`, 32 bytes.
    pub bootstrap_address: u64,
    pub bootstrap_bytes: u32,
    /// Reader body size; its address is the stock allocator's choice.
    pub reader_bytes: u32,
    pub callback_slot: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cleanup {
    pub callback_restored: bool,
    pub bootstrap_restored: bool,
    pub source_state_restored: bool,
    pub ram_gates_unchanged: bool,
    /// The heap block is freed by a confirmed call to the stock release
    /// routine inside the reused path; a failed free makes the pass fail.
    pub allocation_released: bool,
    pub note: &'static str,
}
impl Cleanup {
    fn clean(&self) -> bool {
        self.callback_restored
            && self.bootstrap_restored
            && self.source_state_restored
            && self.ram_gates_unchanged
            && self.allocation_released
    }
    fn residue(&self) -> String {
        [
            (!self.callback_restored, "query callback slot"),
            (!self.bootstrap_restored, "bootstrap slot"),
            (!self.source_state_restored, "bulk source state"),
            (!self.ram_gates_unchanged, "RAM gates"),
            (!self.allocation_released, "heap allocation"),
        ]
        .into_iter()
        .filter(|(bad, _)| *bad)
        .map(|(_, name)| name)
        .collect::<Vec<_>>()
        .join(", ")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ChunkRecord {
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FlashRecord {
    pub device: String,
    pub size: u64,
    pub sha256: String,
    /// Whole-flash reads; both must agree chunk by chunk and in total.
    pub reads: u32,
    pub chunks: Vec<ChunkRecord>,
    pub uncorrectable_page_offsets: Vec<u64>,
    pub upload: UploadRecord,
    pub cleanup: Vec<Cleanup>,
    pub ram_only: &'static str,
}

const RESIDUE_NOTE: &str = "The freed heap block may still hold the reader bytes until the \
    camera reuses it; that is RAM only and clears when the battery comes out.";

/// Values the cleanup must restore, captured before the first upload.
#[derive(Clone, PartialEq, Eq)]
struct UploadArea {
    callback: Vec<u8>,
    bootstrap: Vec<u8>,
    source_state: Vec<u8>,
    gates: [u8; 2],
}

struct Slots {
    callback: u64,
    stock: u32,
    bootstrap: u64,
    source_state: u64,
}

async fn memory(
    session: &mut Session,
    at: u64,
    size: u64,
    limits: DebugMemoryLimits,
) -> Result<Vec<u8>> {
    let address = u32::try_from(at).map_err(|_| refuse("address beyond 32 bits"))?;
    let length = u32::try_from(size)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(refuse("invalid read size"))?;
    Ok(read_range(session, MemoryRange::new(Address(address), length)?, limits).await?)
}
fn le(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .enumerate()
        .fold(0, |n, (i, &b)| n | (u64::from(b) << (8 * i)))
}

impl Inputs<'_> {
    /// Every reason the flash step cannot run, or empty when it can.
    pub(crate) fn flash_refusals(&self) -> Vec<Missing> {
        let mut missing = self.require(&[
            "service_usb",
            "product",
            "internal_version",
            "read_gate",
            "reset_guard",
            "slow_read_bytes",
            "flash_reader",
        ]);
        if self.facts.flash_reader.usable("flash_reader").is_ok() && self.reader.is_none() {
            missing.push(Missing {
                fact: "flash_reader",
                evidence: "pinned, but the reviewed reader files were not embedded and verified"
                    .into(),
            });
        }
        if self.flash.is_none() {
            missing.push(Missing {
                fact: "flash_reader",
                evidence: "the catalog has no xe2-411 flash contract".into(),
            });
        }
        for (name, located, expected) in [
            (
                "read_gate",
                self.facts.read_gate.usable("read_gate").ok(),
                EXR2_READ_GATE,
            ),
            (
                "reset_guard",
                self.facts.reset_guard.usable("reset_guard").ok(),
                EXR2_RESET_GUARD,
            ),
        ] {
            if let Some(byte) = located
                && (byte.offset != expected || byte.enabled_value != 1)
            {
                missing.push(Missing {
                    fact: name,
                    evidence: format!(
                        "located {name} is offset {:#x} = {:#x}, but the reused EXR II reader \
                         path reads offset {expected:#x} and needs 1; the reader may not run \
                         until they agree",
                        byte.offset, byte.enabled_value
                    ),
                });
            }
        }
        missing
    }

    /// What the upload will do, for `plan` and `dry-run`. Sends nothing.
    pub(crate) fn flash_plan(&self) -> Vec<String> {
        let size = self.flash.map_or(0, |f| f.window.size);
        let mut lines = vec![format!(
            "whole flash: {size} bytes of {} through transport {FLASH_MECHANISM} (code runs in RAM only)",
            self.flash.map_or("flash", |f| f.device.kind.as_str()),
        )];
        let Some(reader) = self.reader else {
            lines.push("upload: refused, no reviewed reader is pinned in the facts".into());
            return lines;
        };
        let p = reader.exr2();
        let configuration = match p.configuration {
            Configuration::Fixed { address } => {
                format!("{:#x}", u64::from(address) + BOOTSTRAP_OFFSET)
            }
            Configuration::Pointer { address } => {
                format!("*({address:#x}) + {BOOTSTRAP_OFFSET:#x}")
            }
        };
        let body = reader.image.len() - p.reader_offset as usize;
        lines.extend([
            "repeat identity, require both RAM gates already enabled, re-run stock code checks".into(),
            format!(
                "upload bootstrap: {} bytes (slot {BOOTSTRAP_BYTES}) at {configuration}; original saved and restored",
                p.bootstrap_size
            ),
            format!(
                "upload reader: {body} bytes into a stock heap block from allocate {:#x} \
                 (address chosen by the camera, checked to lie in DRAM and miss protected state); \
                 request at block+{REQUEST:#x}, NAND buffer at block+{BUFFER:#x}",
                p.allocate
            ),
            format!(
                "reader image sha256 {} ({} bytes), artifact sha256 {}",
                reader.pin.image_sha256,
                reader.image.len(),
                reader.pin.artifact_sha256
            ),
            "run: publish the reader in the stock diagnostic-query callback slot; each 0x450006 \
             query reads pages through the stock corrected page reader"
                .into(),
            format!(
                "cleanup: restore callback, free the block via release {:#x}, restore bootstrap and \
                 bulk source, re-check RAM gates; then read them back and compare",
                p.release
            ),
            "read the whole flash twice; both passes must match chunk by chunk".into(),
        ]);
        lines
    }

    fn slots(&self, reader: &Reader) -> Result<Slots> {
        let BulkSource::Query {
            callback_table: _,
            callback_offset: _,
            stock_callback,
            source_state,
            ..
        } = self.bulk.source
        else {
            return Err(refuse("bulk source is not the stock query"));
        };
        let alias = self.bulk.code_alias;
        let bootstrap = match reader.exr2().configuration {
            Configuration::Fixed { address } => u64::from(address) & !alias,
            Configuration::Pointer { .. } => 0,
        } + BOOTSTRAP_OFFSET;
        Ok(Slots {
            callback: 0,
            stock: stock_callback as u32,
            bootstrap,
            source_state: source_state & !alias,
        })
    }

    async fn upload_area(
        &self,
        session: &mut Session,
        reader: &Reader,
        limits: DebugMemoryLimits,
    ) -> Result<(Slots, UploadArea)> {
        let alias = self.bulk.code_alias;
        let BulkSource::Query {
            callback_table,
            callback_offset,
            ..
        } = self.bulk.source
        else {
            return Err(refuse("bulk source is not the stock query"));
        };
        let mut slots = self.slots(reader)?;
        let table = le(&memory(session, callback_table, 4, limits).await?) & !alias;
        slots.callback = table + callback_offset;
        if let Configuration::Pointer { address } = reader.exr2().configuration {
            slots.bootstrap = (le(&memory(session, u64::from(address) | alias, 4, limits).await?)
                & !alias)
                + BOOTSTRAP_OFFSET;
        }
        let gate = self
            .facts
            .read_gate
            .usable("read_gate")
            .map_err(|m| Error::Missing(vec![m]))?;
        let guard = self
            .facts
            .reset_guard
            .usable("reset_guard")
            .map_err(|m| Error::Missing(vec![m]))?;
        let area = UploadArea {
            callback: memory(session, slots.callback, 4, limits).await?,
            bootstrap: memory(session, slots.bootstrap, BOOTSTRAP_BYTES, limits).await?,
            source_state: memory(session, slots.source_state, 8, limits).await?,
            gates: [
                Self::ram_byte(session, gate.offset).await?,
                Self::ram_byte(session, guard.offset).await?,
            ],
        };
        Ok((slots, area))
    }

    async fn check_cleanup(
        &self,
        session: &mut Session,
        reader: &Reader,
        limits: DebugMemoryLimits,
        before: &UploadArea,
        released: bool,
    ) -> Cleanup {
        let unknown = Cleanup {
            callback_restored: false,
            bootstrap_restored: false,
            source_state_restored: false,
            ram_gates_unchanged: false,
            allocation_released: false,
            note: "USB completion is unknown, so nothing could be read back. Pull the battery: \
                   the upload area is RAM only.",
        };
        if session.state() != SessionState::Ready {
            return unknown;
        }
        match self.upload_area(session, reader, limits).await {
            Ok((slots, after)) => Cleanup {
                callback_restored: after.callback == before.callback
                    && le(&after.callback) == u64::from(slots.stock),
                bootstrap_restored: after.bootstrap == before.bootstrap,
                source_state_restored: after.source_state == before.source_state,
                ram_gates_unchanged: after.gates == before.gates,
                allocation_released: released,
                note: RESIDUE_NOTE,
            },
            Err(_) => unknown,
        }
    }

    /// Back up the whole flash with the pinned reader. `sink(offset, bytes)`
    /// receives the first read's chunks (in the reader's order, not offset
    /// order). The RAM gates must already be enabled by [`Inputs::capture`];
    /// this never writes configuration.
    pub async fn back_up_flash(
        &self,
        session: &mut Session,
        usb: UsbIds,
        sink: &mut dyn FnMut(u64, &[u8]) -> std::result::Result<(), String>,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<FlashRecord> {
        // Refuse before any USB command.
        let missing = self.flash_refusals();
        if !missing.is_empty() {
            return Err(Error::Missing(missing));
        }
        self.validate()?;
        let (Some(reader), Some(flash)) = (self.reader, self.flash) else {
            unreachable!("refusals cover the reader and flash contract");
        };
        let limits = self.limits()?;
        let workflow = fujimod_workflows::Limits {
            memory: limits,
            configuration: NonZeroU32::MIN,
        };
        // Identity and stock code before anything is uploaded.
        self.identify(session, usb).await?;
        let gate = self.facts.read_gate.usable("read_gate").expect("ready");
        let guard = self.facts.reset_guard.usable("reset_guard").expect("ready");
        for (name, byte) in [("read gate", gate), ("reset guard", guard)] {
            let value = Self::ram_byte(session, byte.offset).await?;
            if value != byte.enabled_value {
                return Err(refuse(format!(
                    "{name} is {value:#x}, not enabled; run the state capture first (this step \
                     never writes configuration)"
                )));
            }
        }
        self.verify_code(session, limits).await?;
        let (slots, before) = self.upload_area(session, reader, limits).await?;
        if le(&before.callback) != u64::from(slots.stock) {
            return Err(refuse(
                "the diagnostic-query callback is not stock; nothing uploaded",
            ));
        }
        let upload = UploadRecord {
            mechanism: FLASH_MECHANISM,
            artifact_sha256: reader.pin.artifact_sha256.clone(),
            image_sha256: reader.pin.image_sha256.clone(),
            image_bytes: reader.pin.image_bytes,
            review: reader.pin.review.clone(),
            bootstrap_address: slots.bootstrap,
            bootstrap_bytes: reader.exr2().bootstrap_size,
            reader_bytes: (reader.image.len() - reader.exr2().reader_offset as usize) as u32,
            callback_slot: slots.callback,
        };
        let mut first: BTreeMap<u64, (u64, String)> = BTreeMap::new();
        let mut cleanups = Vec::new();
        let mut backups = Vec::new();
        for pass in 0..2u32 {
            let mut seen: BTreeMap<u64, (u64, String)> = BTreeMap::new();
            let mut chunk = |offset: u64, bytes: Vec<u8>| -> std::result::Result<(), String> {
                let digest = sha(&bytes);
                if seen
                    .insert(offset, (bytes.len() as u64, digest.clone()))
                    .is_some()
                {
                    return Err(format!("flash chunk {offset:#x} delivered twice"));
                }
                if pass == 0 {
                    sink(offset, &bytes)
                } else if first.get(&offset) != Some(&(bytes.len() as u64, digest)) {
                    Err(format!("flash reads differ in the chunk at {offset:#x}"))
                } else {
                    Ok(())
                }
            };
            let mut report = |done: u64, total: u64| {
                progress(u64::from(pass) * total + done, 2 * total);
                Ok(())
            };
            let result = fujimod_workflows::flash::backup(
                session,
                self.bulk,
                flash,
                &reader.payload,
                &reader.image,
                workflow,
                &mut Callbacks {
                    chunk: &mut chunk,
                    progress: &mut report,
                },
            )
            .await;
            let cleanup = self
                .check_cleanup(session, reader, limits, &before, result.is_ok())
                .await;
            let clean = cleanup.clean();
            let residue = cleanup.residue();
            cleanups.push(cleanup);
            match result {
                Err(error) => {
                    return Err(Error::Flash(format!(
                        "read {} failed: {error}. Cleanup check: {}",
                        pass + 1,
                        if clean {
                            "upload area restored".to_string()
                        } else {
                            format!("NOT restored: {residue}; pull the battery (RAM only)")
                        }
                    )));
                }
                Ok(_) if !clean => {
                    return Err(Error::Cleanup(format!(
                        "after read {}: {residue} differ from before the upload; pull the \
                         battery (RAM only)",
                        pass + 1
                    )));
                }
                Ok(backup) => backups.push(backup),
            }
            if pass == 0 {
                first = seen;
            } else if seen.len() != first.len() {
                return Err(Error::Flash(
                    "flash reads delivered different chunks".into(),
                ));
            }
        }
        let (a, b) = (&backups[0], &backups[1]);
        if a.sha256 != b.sha256 || a.size != b.size || a.size != flash.window.size {
            return Err(Error::Flash(format!(
                "the two flash reads differ: {} vs {}",
                a.sha256, b.sha256
            )));
        }
        if a.uncorrectable_page_offsets != b.uncorrectable_page_offsets {
            return Err(Error::Flash(
                "the two flash reads flag different uncorrectable pages".into(),
            ));
        }
        let covered: u64 = first.values().map(|(n, _)| n).sum();
        if covered != flash.window.size {
            return Err(Error::Flash("flash chunks do not cover the device".into()));
        }
        Ok(FlashRecord {
            device: flash.device.kind.clone(),
            size: a.size,
            sha256: a.sha256.clone(),
            reads: 2,
            chunks: first
                .into_iter()
                .map(|(offset, (size, sha256))| ChunkRecord {
                    offset,
                    size,
                    sha256,
                })
                .collect(),
            uncorrectable_page_offsets: a.uncorrectable_page_offsets.clone(),
            upload,
            cleanup: cleanups,
            ram_only: "The reader ran from a heap block in RAM. No flash erase, program or \
                       write request was sent, no stored setting was written, and nothing \
                       persists on the camera.",
        })
    }

    /// The command allowance for the flash phase, for the wire and the audit.
    pub fn flash_policy(&self) -> policy::FlashPolicy {
        let alias = self.bulk.code_alias;
        let mut protected: Vec<(u64, u64)> = self
            .bulk
            .code_checks
            .iter()
            .map(|c| (c.address, c.size))
            .chain(self.bulk.mapped_probe.iter().map(|c| (c.address, c.size)))
            .chain(
                self.flash
                    .iter()
                    .flat_map(|f| f.code_ranges.iter().map(|r| (r.address, r.size))),
            )
            .chain(
                self.reader
                    .iter()
                    .flat_map(|r| r.exr2().checks.iter().map(|c| (c.address, c.size))),
            )
            .map(|(address, size)| (address & !alias, (address & !alias) + size))
            .collect();
        protected.sort_unstable();
        policy::FlashPolicy {
            dram: (
                self.bulk.window.address & !alias,
                (self.bulk.window.address & !alias) + self.bulk.window.size,
            ),
            alias,
            protected,
            max_write: NonZeroU32::MIN.get(),
        }
    }
}
