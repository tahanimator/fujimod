//! Safety-gate tests against a stateful mock FF80 camera. The mock applies the
//! write policy to every command and control request it receives.
use super::*;
use crate::{
    facts::{Confidence, Fact},
    policy::{Auditor, CommandCheck, Kind, Policy, check_control},
};
use fujimod_usb::{
    ff80::framing::RequestKind,
    testing::{Peer, peer_session},
};
use futures_lite::future::block_on;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const ALIAS: u32 = 0x4000_0000;
const GATE: RamByte = RamByte {
    offset: 0x20,
    enabled_value: 1,
};
const GUARD: RamByte = RamByte {
    offset: 0x21,
    enabled_value: 1,
};
const TABLE_POINTER: u32 = 0x4170_0000;
const CAPACITY: u32 = 1 << 20;

fn physical(address: u32) -> u32 {
    if address >= 0x8000_0000 {
        address
    } else {
        address & !ALIAS
    }
}
fn pattern(address: u32) -> u8 {
    let a = physical(address);
    (a ^ (a >> 8) ^ (a >> 16) ^ (a >> 24)) as u8 ^ 0x5a
}

#[derive(Default)]
struct Camera {
    product: String,
    version: String,
    configuration: Vec<u8>,
    overrides: HashMap<u32, u8>,
    corrupt: Option<u32>,
    /// Refuse debug memory reads while the test read gate is 0.
    gated: bool,
    reply: Vec<u8>,
    policy: Option<Box<dyn CommandCheck>>,
    violations: Vec<String>,
    /// EXR II reader emulation; `None` keeps the stock-only camera.
    exr2: Option<Exr2>,
    configuration_writes: Vec<(u32, u8)>,
    commands: Vec<u32>,
    base: u32,
    source_state: u32,
    callback_offset: u32,
    stock_callback: u32,
}
impl Camera {
    fn byte(&self, address: u32) -> u8 {
        if self.corrupt == Some(physical(address)) {
            return pattern(address) ^ 0xff;
        }
        *self
            .overrides
            .get(&physical(address))
            .unwrap_or(&pattern(address))
    }
    fn bytes(&self, address: u32, size: u32) -> Vec<u8> {
        (0..size)
            .map(|n| self.byte(address.wrapping_add(n)))
            .collect()
    }
    fn put_word(&mut self, address: u32, value: u32) {
        for (n, b) in value.to_le_bytes().into_iter().enumerate() {
            self.overrides.insert(physical(address) + n as u32, b);
        }
    }
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn info(text: &str) -> Vec<u8> {
    let mut reply = vec![0; 16];
    reply[..4].copy_from_slice(&policy::DEVICE_INFO.to_le_bytes());
    for unit in text.encode_utf16().chain([0]) {
        reply.extend(unit.to_le_bytes());
    }
    reply
}

struct Device(Arc<Mutex<Camera>>);
impl Peer for Device {
    fn control(&mut self, kind: RequestKind, envelope: [u8; 12]) -> fujimod_usb::Result<()> {
        let mut c = self.0.lock().unwrap();
        if let Err(v) = check_control(kind) {
            c.violations.push(v.to_string());
        }
        match kind {
            RequestKind::Open => c.reply = vec![0x34, 0x12, 0, 0, 0, 0, 1, 0],
            RequestKind::ReadData => {
                let offset = u32::from_be_bytes(envelope[2..6].try_into().unwrap());
                let size = u32::from_be_bytes(envelope[6..10].try_into().unwrap());
                c.reply = c.bytes(c.base.wrapping_add(offset), size);
            }
            _ => {}
        }
        Ok(())
    }
    fn write(&mut self, raw: &[u8]) -> fujimod_usb::Result<()> {
        let mut c = self.0.lock().unwrap();
        let command = word(raw, 0);
        c.commands.push(command);
        let checked = c.policy.as_ref().expect("camera policy").check(raw);
        match checked {
            Err(v) => {
                c.violations.push(v.to_string());
                c.reply = raw[..raw.len().min(36)].to_vec();
                return Ok(());
            }
            Ok(Kind::RamGateWrite { offset, value }) => {
                c.configuration_writes.push((offset, value));
            }
            Ok(Kind::Read) => {}
            Ok(Kind::RamWrite { address, length }) => {
                for n in 0..length {
                    let b = raw[36 + n as usize];
                    c.overrides.insert(physical(address + n), b);
                }
                c.reply = raw[..36].to_vec();
                return Ok(());
            }
            Ok(Kind::Execute { argument }) => {
                let callback = c.word_at(c.callback_slot());
                if callback != c.stock_callback {
                    c.reply = c.execute(callback, argument);
                    return Ok(());
                }
            }
        }
        let function = word(raw, 16);
        c.reply = match (command, function) {
            (policy::DEVICE_INFO, 0x04) => info(&c.product.clone()),
            (policy::DEVICE_INFO, 0x08) => info(&c.version.clone()),
            (policy::DEVICE_INFO, _) => info("SERIAL-TEST"),
            (policy::CONFIGURATION, 1) => {
                let mut r = raw[..36].to_vec();
                r[28..32].copy_from_slice(&(c.configuration.len() as u32).to_le_bytes());
                r[32..36].copy_from_slice(&256u32.to_le_bytes());
                r
            }
            (policy::CONFIGURATION, 2) => {
                let (offset, n) = (word(raw, 28) as usize, word(raw, 32) as usize);
                let mut r = raw[..36].to_vec();
                r.extend_from_slice(&c.configuration[offset..offset + n]);
                r
            }
            (policy::CONFIGURATION, 3) => {
                let offset = word(raw, 28) as usize;
                c.configuration[offset] = raw[36];
                raw[..36].to_vec()
            }
            (policy::MEMORY, 2) if c.gated && c.configuration[GATE.offset as usize] == 0 => {
                let mut r = raw[..36].to_vec();
                r[4] = 1;
                r
            }
            (policy::MEMORY, 2) => {
                let mut r = raw[..36].to_vec();
                r.extend(c.bytes(word(raw, 28), word(raw, 32)));
                r
            }
            (policy::DIAGNOSTIC_QUERY, _) => {
                let (state, base) = (c.source_state, c.base);
                c.put_word(state, base);
                c.put_word(state + 4, CAPACITY);
                let mut r = vec![0; 32];
                r[..4].copy_from_slice(&command.to_le_bytes());
                r[16..20].copy_from_slice(&base.to_le_bytes());
                r[20..24].copy_from_slice(&CAPACITY.to_le_bytes());
                r
            }
            _ => unreachable!("policy admitted {command:#x}"),
        };
        Ok(())
    }
    fn read(&mut self, maximum: usize) -> fujimod_usb::Result<Vec<u8>> {
        let c = self.0.lock().unwrap();
        assert!(c.reply.len() <= maximum);
        Ok(c.reply.clone())
    }
}

/// The real xe2-411 bulk geometry with digests of the mock's memory.
fn bulk() -> FirmwareBulk {
    let mut doc: serde_json::Value = serde_json::from_str(include_str!(
        "../../../catalog/firmware/xe2/4.11/bulk.generated.json"
    ))
    .unwrap();
    let alias = doc["code_alias"].as_u64().unwrap() as u32;
    let rehash = |check: &mut serde_json::Value| {
        let (a, n) = (
            check["address"].as_u64().unwrap() as u32 | alias,
            check["size"].as_u64().unwrap() as u32,
        );
        let bytes: Vec<u8> = (0..n).map(|i| pattern(a + i)).collect();
        check["sha256"] = hex(&checks::sha256_bytes(&bytes)).into();
    };
    for check in doc["code_checks"].as_array_mut().unwrap() {
        rehash(check);
    }
    rehash(&mut doc["mapped_probe"]);
    serde_json::from_value(doc).unwrap()
}
fn state() -> FirmwareState {
    serde_json::from_value(serde_json::json!({
        "$schema": "https://fujimod.com/schemas/state.generated.schema.json",
        "schema": 1, "id": "xe2-411-state", "firmware_id": "xe2-411",
        "regions": [
            {"name": "dram", "address": 0x4010_0000u32, "size": 0x8000, "constant": false},
            {"name": "boot-rom", "address": 0x8000_0000u32, "size": 0x4000, "constant": true}
        ]
    }))
    .unwrap()
}
fn high<T>(value: T) -> Fact<T> {
    Fact {
        value: Some(value),
        confidence: Confidence::High,
        evidence: "test".into(),
    }
}
/// Bundled facts with the unlocated gate facts filled in for tests only.
fn located() -> ServiceFacts {
    let mut f = ServiceFacts::bundled();
    f.read_gate = high(GATE);
    f.reset_guard = high(GUARD);
    f.slow_read_bytes = high(256);
    f
}
const USB: UsbIds = UsbIds {
    vendor_id: 0x04cb,
    product_id: 0xff80,
};

fn camera(policy: Policy) -> Arc<Mutex<Camera>> {
    let bulk = bulk();
    let BulkSource::Query {
        callback_table,
        callback_offset,
        stock_callback,
        base,
        source_state,
        ..
    } = bulk.source
    else {
        unreachable!()
    };
    let mut c = Camera {
        product: "X-E2".into(),
        version: "3.05".into(),
        configuration: vec![0; 0x1000],
        policy: Some(Box::new(policy)),
        base: base as u32,
        callback_offset: callback_offset as u32,
        stock_callback: stock_callback as u32,
        source_state: (source_state as u32) & !ALIAS,
        ..Camera::default()
    };
    c.put_word(callback_table as u32, TABLE_POINTER);
    c.put_word(
        TABLE_POINTER + callback_offset as u32,
        stock_callback as u32,
    );
    Arc::new(Mutex::new(c))
}
fn write_policy() -> Policy {
    Policy {
        read_gate: Some(GATE),
        reset_guard: Some(GUARD),
    }
}

struct Run {
    result: Result<Manifest>,
    saved: Vec<(String, Vec<u8>)>,
}
fn capture(facts: &ServiceFacts, c: &Arc<Mutex<Camera>>, usb: UsbIds) -> Run {
    let (bulk, state) = (bulk(), state());
    let inputs = Inputs {
        facts,
        bulk: &bulk,
        state: &state,
        flash: None,
        reader: None,
    };
    let mut saved = Vec::new();
    let result = block_on(async {
        let mut session = peer_session(Device(c.clone()), 2 << 20, None)
            .await
            .unwrap();
        inputs
            .capture(
                &mut session,
                usb,
                &mut |name, bytes| {
                    saved.push((name.to_string(), bytes.to_vec()));
                    Ok(format!("{name}.bin"))
                },
                &mut |_, _| {},
            )
            .await
    });
    Run { result, saved }
}

#[test]
fn bundled_facts_refuse_capture_and_flash_naming_the_missing_facts() {
    let facts = ServiceFacts::bundled();
    let c = camera(Policy::read_only());
    let run = capture(&facts, &c, USB);
    let Err(Error::Missing(missing)) = run.result else {
        panic!("capture must refuse");
    };
    let names: Vec<_> = missing.iter().map(|m| m.fact).collect();
    assert_eq!(names, ["read_gate", "reset_guard", "slow_read_bytes"]);
    // Refused before any FF80 command.
    assert!(c.lock().unwrap().commands.is_empty());
    let (bulk, state) = (bulk(), state());
    let plan = Inputs {
        facts: &facts,
        bulk: &bulk,
        state: &state,
        flash: None,
        reader: None,
    }
    .plan();
    let flash = plan
        .iter()
        .find(|s| s.name.starts_with("back up flash"))
        .unwrap();
    assert!(!flash.ready);
    let names: Vec<_> = flash.refused_because.iter().map(|m| m.fact).collect();
    assert_eq!(
        names,
        [
            "read_gate",
            "reset_guard",
            "slow_read_bytes",
            "flash_reader",
            "flash_reader"
        ]
    );
    assert!(plan.iter().find(|s| s.name == "identify").unwrap().ready);
}

#[test]
fn successful_capture_writes_only_the_two_ram_gates() {
    for gated in [false, true] {
        successful_capture(gated);
    }
}
fn successful_capture(gated: bool) {
    let c = camera(write_policy());
    c.lock().unwrap().gated = gated;
    let run = capture(&located(), &c, USB);
    let manifest = run.result.expect("capture");
    assert_eq!(manifest.code_verified_before_any_write, !gated);
    let camera = c.lock().unwrap();
    assert!(camera.violations.is_empty(), "{:?}", camera.violations);
    assert_eq!(camera.configuration_writes, [(0x20, 1), (0x21, 1)]);
    assert_eq!(manifest.gates[0].original, 0);
    assert!(manifest.gates.iter().all(|g| g.written));
    let names: Vec<_> = run.saved.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["dram", "boot-rom"]);
    for (record, (_, bytes)) in manifest.regions.iter().zip(&run.saved) {
        assert_eq!(
            record.sha256.as_deref(),
            Some(hex(&checks::sha256_bytes(bytes)).as_str())
        );
        assert_eq!(
            bytes,
            &camera.bytes(record.address as u32, record.size as u32)
        );
    }
    assert_eq!(manifest.regions[1].reads, 2);
    assert_eq!(manifest.identity.serial.as_deref(), Some("SERIAL-TEST"));
}

#[test]
fn already_enabled_gates_are_not_rewritten() {
    let c = camera(write_policy());
    c.lock().unwrap().configuration[0x20] = 1;
    c.lock().unwrap().configuration[0x21] = 1;
    capture(&located(), &c, USB).result.expect("capture");
    assert!(c.lock().unwrap().configuration_writes.is_empty());
}

#[test]
fn identity_mismatches_abort_before_the_first_write() {
    let cases: [(&str, Box<dyn Fn(&mut Camera)>, UsbIds); 3] = [
        (
            "usb",
            Box::new(|_| {}),
            UsbIds {
                vendor_id: 0x04cb,
                product_id: 0x02e5,
            },
        ),
        ("product", Box::new(|c| c.product = "X-T1".into()), USB),
        ("version", Box::new(|c| c.version = "3.04".into()), USB),
    ];
    for (name, change, usb) in cases {
        let c = camera(write_policy());
        change(&mut c.lock().unwrap());
        let run = capture(&located(), &c, usb);
        assert!(
            matches!(run.result, Err(Error::Identity(_))),
            "{name}: {:?}",
            run.result.err()
        );
        let camera = c.lock().unwrap();
        assert!(camera.configuration_writes.is_empty(), "{name}");
        assert!(!camera.commands.contains(&policy::MEMORY), "{name}");
    }
}

#[test]
fn unexpected_gate_value_aborts_before_writing() {
    let c = camera(write_policy());
    c.lock().unwrap().configuration[0x21] = 7;
    let run = capture(&located(), &c, USB);
    assert!(matches!(run.result, Err(Error::Identity(_))));
    assert!(c.lock().unwrap().configuration_writes.is_empty());
}

#[test]
fn changed_stock_code_aborts_before_any_write_when_reads_are_open() {
    let c = camera(write_policy());
    let first = bulk().code_checks[3].address as u32;
    c.lock().unwrap().corrupt = Some(first + 5);
    let run = capture(&located(), &c, USB);
    assert!(matches!(run.result, Err(Error::CodeDiffers(_))));
    assert!(c.lock().unwrap().configuration_writes.is_empty());
    assert!(run.saved.is_empty());
}

#[test]
fn changed_stock_code_restores_the_read_gate_and_never_arms_the_guard() {
    let c = camera(write_policy());
    let first = bulk().code_checks[3].address as u32;
    c.lock().unwrap().corrupt = Some(first + 5);
    c.lock().unwrap().gated = true;
    let run = capture(&located(), &c, USB);
    assert!(matches!(run.result, Err(Error::CodeDiffers(_))));
    let camera = c.lock().unwrap();
    assert_eq!(camera.configuration_writes, [(0x20, 1), (0x20, 0)]);
    assert_eq!(camera.configuration[0x21], 0);
    assert!(run.saved.is_empty());
    assert!(camera.violations.is_empty());
}

#[test]
fn dry_run_sends_no_write() {
    for facts in [ServiceFacts::bundled(), located()] {
        let c = camera(Policy::read_only());
        c.lock().unwrap().gated = true;
        let (bulk, state) = (bulk(), state());
        let inputs = Inputs {
            facts: &facts,
            bulk: &bulk,
            state: &state,
            flash: None,
            reader: None,
        };
        let trace = fujimod_usb::trace::Trace::new(
            std::num::NonZeroU32::new(4096).unwrap(),
            std::num::NonZeroU32::new(1 << 20).unwrap(),
        );
        let report = block_on(async {
            let mut session = peer_session(Device(c.clone()), 2 << 20, Some(trace.clone()))
                .await
                .unwrap();
            inputs.dry_run(&mut session, USB).await
        })
        .expect("dry run");
        let camera = c.lock().unwrap();
        assert!(camera.violations.is_empty(), "{:?}", camera.violations);
        assert!(camera.configuration_writes.is_empty());
        assert!(report.code_checks.starts_with("not run"));
        let mut auditor = Auditor::new(Policy::read_only());
        auditor.absorb(trace.drain());
        assert!(auditor.finish().unwrap() >= 5);
    }
}

#[test]
fn dry_run_checks_code_when_reads_are_allowed() {
    let c = camera(Policy::read_only());
    let facts = located();
    let (bulk, state) = (bulk(), state());
    let inputs = Inputs {
        facts: &facts,
        bulk: &bulk,
        state: &state,
        flash: None,
        reader: None,
    };
    let report = block_on(async {
        let mut session = peer_session(Device(c.clone()), 2 << 20, None)
            .await
            .unwrap();
        inputs.dry_run(&mut session, USB).await
    })
    .unwrap();
    assert_eq!(report.code_checks, "15 regions match stock");
    assert!(c.lock().unwrap().violations.is_empty());
}

fn command(code: u32, function: u32, params: [u32; 4], data: &[u8]) -> Vec<u8> {
    let mut raw = vec![0; 16];
    raw[..4].copy_from_slice(&code.to_le_bytes());
    raw.extend(function.to_le_bytes());
    for p in params {
        raw.extend(p.to_le_bytes());
    }
    raw.extend_from_slice(data);
    raw
}

#[test]
fn policy_blocks_every_write_except_the_two_ram_gates() {
    let p = write_policy();
    let ram = |offset, value| command(policy::CONFIGURATION, 3, [1, 0, offset, 1], &[value]);
    assert_eq!(
        p.check(&ram(0x20, 1)),
        Ok(Kind::RamGateWrite {
            offset: 0x20,
            value: 1
        })
    );
    assert!(p.check(&ram(0x20, 0)).is_ok(), "read gate restore");
    assert!(p.check(&ram(0x21, 0)).is_err(), "guard is never cleared");
    assert!(p.check(&ram(0x20, 2)).is_err());
    assert!(p.check(&ram(0x22, 1)).is_err(), "other RAM bytes");
    let stored = command(policy::CONFIGURATION, 3, [0, 0, 0x20, 1], &[1]);
    assert!(p.check(&stored).is_err(), "stored configuration");
    let page = command(policy::CONFIGURATION, 3, [1, 1, 0x20, 1], &[1]);
    assert!(p.check(&page).is_err());
    let wide = command(policy::CONFIGURATION, 3, [1, 0, 0x20, 2], &[1, 1]);
    assert!(p.check(&wide).is_err());
    let memory_write = command(policy::MEMORY, 3, [0, 0, 0x4000_0000, 1], &[0]);
    assert!(p.check(&memory_write).is_err());
    assert!(Policy::read_only().check(&ram(0x20, 1)).is_err());
    for code in [0x660002, 0x550001, 0x760001, 0x110000, 0x2a0001] {
        assert!(
            p.check(&command(code, 0, [0; 4], &[])).is_err(),
            "{code:#x}"
        );
    }
    let mut other_query = vec![0; 32];
    other_query[..4].copy_from_slice(&policy::DIAGNOSTIC_QUERY.to_le_bytes());
    other_query[16] = 0x81;
    assert!(p.check(&other_query).is_err());
    assert!(check_control(RequestKind::WriteData).is_err());
    assert!(check_control(RequestKind::ReadData).is_ok());
    // Reads stay allowed.
    let read = command(policy::MEMORY, 2, [0, 0, 0x8000_0000, 256], &[]);
    assert_eq!(p.check(&read), Ok(Kind::Read));
}

#[test]
fn crate_source_has_one_write_call_site() {
    let source = include_str!("lib.rs");
    let forbidden = [
        "write_memory",
        "write_range",
        "write_data",
        "prepare_data_write",
        "send_event",
        "submit_console",
        "research_command",
        "fujimod_workflows::flash",
        "debug_mode::",
        "restart::",
        "install::",
        "service_mode::",
    ];
    for name in forbidden {
        assert!(!source.contains(name), "lib.rs mentions {name}");
    }
    assert_eq!(source.matches("write_byte_verified(").count(), 1);
    assert_eq!(source.matches("WriteConfiguration::new(").count(), 1);
    let main = include_str!("main.rs");
    for name in forbidden {
        assert!(!main.contains(name), "main.rs mentions {name}");
    }
}

#[test]
fn facts_reject_unsafe_shapes() {
    let mut doc: serde_json::Value = serde_json::from_str(facts::XE2_411).unwrap();
    doc["firmware_id"] = "x70-110".into();
    assert!(ServiceFacts::from_json(&doc.to_string()).is_err());
    let mut doc: serde_json::Value = serde_json::from_str(facts::XE2_411).unwrap();
    doc["read_gate"]["value"] = serde_json::json!({"offset": 5, "enabled_value": 1});
    doc["reset_guard"]["value"] = serde_json::json!({"offset": 5, "enabled_value": 1});
    assert!(ServiceFacts::from_json(&doc.to_string()).is_err());
    let mut doc: serde_json::Value = serde_json::from_str(facts::XE2_411).unwrap();
    doc["read_gate"]["value"] = serde_json::json!({"offset": 5, "enabled_value": 1});
    let facts = ServiceFacts::from_json(&doc.to_string()).unwrap();
    assert!(
        facts.read_gate.usable("read_gate").is_err(),
        "unlocated confidence"
    );
}

// ---------------------------------------------------------------------------
// Flash backup through the reused EXR II reader path (U13).
// ---------------------------------------------------------------------------
use crate::{
    facts::FlashReader,
    flash::{EXR2_READ_GATE, EXR2_RESET_GUARD, FlashRecord, Reader},
    reader_scan,
};
use fujimod_workflows::flash::{
    Artifact, CodeCheck, Payload,
    exr2::{self, Configuration},
};

/// The camera side of the EXR II reader ABI: the stock allocator and release
/// reached through the bootstrap stub, the cache primitives, and the uploaded
/// reader filling its request from emulated NAND. There is no NAND write path.
#[derive(Default)]
struct Exr2 {
    nand: Vec<u8>,
    bootstrap: u32,
    allocate: u32,
    release: u32,
    cache: Vec<u32>,
    stock_reader: u32,
    /// The reader body the test pinned; a block holding anything else never runs.
    body: Vec<u8>,
    heap: u32,
    allocations: Vec<u32>,
    released: Vec<u32>,
    reader_runs: usize,
    stock_entries: Vec<u32>,
    executed: Vec<u32>,
    refuse_release: bool,
}

impl Camera {
    fn word_at(&self, address: u32) -> u32 {
        word(&self.bytes(address, 4), 0)
    }
    fn callback_slot(&self) -> u32 {
        physical(TABLE_POINTER) + self.callback_offset
    }
    fn put_bytes(&mut self, address: u32, bytes: &[u8]) {
        for (n, &b) in bytes.iter().enumerate() {
            self.overrides.insert(physical(address) + n as u32, b);
        }
    }
    /// One diagnostic query while a non-stock callback is published.
    fn execute(&mut self, callback: u32, argument: u32) -> Vec<u8> {
        let mut e = self
            .exr2
            .take()
            .expect("EXR II emulation for a published callback");
        e.executed.push(callback);
        let outputs = self.source_state;
        let ok = if e.cache.contains(&callback) {
            true
        } else if callback == e.bootstrap {
            let function = self.word_at(outputs + 4);
            if function == e.allocate {
                let block = e.heap + e.allocations.len() as u32 * 0x10_0000;
                e.allocations.push(block);
                self.put_word(outputs, block);
                true
            } else if function == e.release {
                if !e.refuse_release {
                    e.released.push(argument);
                }
                !e.refuse_release
            } else {
                self.violations
                    .push(format!("bootstrap called unpinned {function:#x}"));
                false
            }
        } else if e.allocations.last() == Some(&callback) && !e.released.contains(&callback) {
            if self.bytes(callback, e.body.len() as u32) != e.body {
                self.violations
                    .push("block holds an unpinned reader".into());
                false
            } else {
                e.reader_runs += 1;
                let request = physical(argument);
                let w = |c: &Camera, i: u32| c.word_at(request + 4 * i);
                let (offset, size, buffer, stock) =
                    (w(self, 1), w(self, 2), w(self, 3), w(self, 4));
                e.stock_entries.push(stock);
                let data = e.nand[offset as usize..(offset + size) as usize].to_vec();
                self.put_bytes(buffer, &data);
                let calls = w(self, 11) + 1;
                for (at, value) in [(40, 0), (44, calls), (48, offset), (52, size), (56, size)] {
                    self.put_word(request + at, value);
                }
                let pages = size / 2048;
                let bitmap = pages.div_ceil(32) * 4;
                self.put_bytes(request + 76, &vec![0; 16 + bitmap as usize]);
                true
            }
        } else {
            self.violations
                .push(format!("query ran unpinned entry {callback:#x}"));
            false
        };
        self.exr2 = Some(e);
        let mut reply = vec![0; 32];
        reply[..4].copy_from_slice(&policy::DIAGNOSTIC_QUERY.to_le_bytes());
        if !ok {
            reply[4] = 1;
        }
        reply
    }
}

const FLASH_BYTES: u64 = 0x8_0000;
const PRIMITIVES: u64 = 0x0100_0000;
const CONFIGURATION: u32 = 0x0150_0000;
const HEAP: u32 = 0x0800_0000;
const NOP: u32 = 0xe1a0_0000;
const BRANCH_SELF: u32 = 0xeaff_fffe;

fn nand(offset: u64) -> u8 {
    ((offset >> 11) as u8).wrapping_mul(31) ^ (offset as u8) ^ 0xa5
}
fn nand_bytes() -> Vec<u8> {
    (0..FLASH_BYTES).map(nand).collect()
}
/// The real xe2-411 flash contract, shrunk to a small device whose control
/// bytes are the mock's NAND.
fn test_flash() -> FirmwareFlash {
    let mut doc: serde_json::Value = serde_json::from_str(include_str!(
        "../../../catalog/firmware/xe2/4.11/flash.generated.json"
    ))
    .unwrap();
    let data = nand_bytes();
    doc["window"]["size"] = FLASH_BYTES.into();
    doc["control"]["sha256"] = hex(&checks::sha256_bytes(&data[0x4_0000..0x4_8000])).into();
    serde_json::from_value(doc).unwrap()
}
fn stock_sha(address: u64, size: u64) -> String {
    let bytes: Vec<u8> = (0..size as u32)
        .map(|i| pattern((address as u32 | ALIAS) + i))
        .collect();
    hex(&checks::sha256_bytes(&bytes))
}
fn words_le(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
/// Bootstrap `bx lr` padded to the slot, then `body`, then the observer.
fn image_with(body: &[u32]) -> Vec<u8> {
    let mut image = words_le(&[reader_scan::BX_LR]);
    image.resize(32, 0);
    image.extend(words_le(body));
    image.extend(words_le(&[reader_scan::BX_LR]));
    image
}
fn good_image() -> Vec<u8> {
    image_with(&[NOP, NOP, BRANCH_SELF])
}
/// Artifact, image and pin for `image`, built the way build::prepare_exr2 lays
/// out a payload (test fixture only; no camera code is authored here).
fn reader_parts(image: &[u8], flash: &FirmwareFlash) -> (FlashReader, Vec<u8>) {
    let p = PRIMITIVES as u32;
    let mut checks: Vec<CodeCheck> = flash
        .code_ranges
        .iter()
        .map(|r| CodeCheck {
            address: r.address,
            size: r.size,
            sha256: stock_sha(r.address, r.size),
        })
        .collect();
    checks.push(CodeCheck {
        address: PRIMITIVES,
        size: 0x40,
        sha256: stock_sha(PRIMITIVES, 0x40),
    });
    let payload = exr2::Payload {
        schema: 6,
        firmware_id: "xe2-411".into(),
        firmware_sha256: ServiceFacts::bundled().firmware_sha256,
        sha256: hex(&checks::sha256_bytes(image)),
        bootstrap_size: 4,
        reader_offset: 32,
        observer_offset: image.len() as u32 - 4,
        allocate: p,
        release: p + 4,
        configuration: Configuration::Fixed {
            address: CONFIGURATION,
        },
        cache_clean: p + 8,
        instruction_invalidate: p + 12,
        barrier: p + 16,
        instruction_barrier: p + 20,
        checks,
        control_sha256: flash.control.sha256.clone(),
        update_size: FLASH_BYTES,
    };
    let artifact = serde_json::to_vec(&Artifact {
        payload: Payload::Exr2Query(payload),
        image: Some("reader.bin".into()),
    })
    .unwrap();
    let pin = FlashReader {
        mechanism: "exr2-query-heap".into(),
        artifact: "reader.json".into(),
        artifact_sha256: hex(&checks::sha256_bytes(&artifact)),
        image: "reader.bin".into(),
        image_sha256: hex(&checks::sha256_bytes(image)),
        image_bytes: image.len() as u32,
        review: "test fixture".into(),
    };
    (pin, artifact)
}
/// Located facts that agree with the reused reader's RAM-gate offsets, and a
/// pinned reader.
fn flash_facts(pin: &FlashReader) -> ServiceFacts {
    let mut f = located();
    f.read_gate = high(RamByte {
        offset: EXR2_READ_GATE,
        enabled_value: 1,
    });
    f.reset_guard = high(RamByte {
        offset: EXR2_RESET_GUARD,
        enabled_value: 1,
    });
    f.flash_reader = high(pin.clone());
    f
}
fn verified(facts: &ServiceFacts, flash: &FirmwareFlash, image: &[u8]) -> Result<Reader> {
    let (pin, artifact) = reader_parts(image, flash);
    Reader::verify(&pin, &artifact, image, facts, flash, &bulk())
}
fn flash_camera(facts: &ServiceFacts, flash: &FirmwareFlash) -> Arc<Mutex<Camera>> {
    let c = camera(Policy {
        read_gate: facts.read_gate.value,
        reset_guard: facts.reset_guard.value,
    });
    {
        let mut camera = c.lock().unwrap();
        for check in &flash.state_checks {
            camera.put_word(check.address as u32, check.value as u32);
        }
        let p = PRIMITIVES as u32;
        camera.exr2 = Some(Exr2 {
            nand: nand_bytes(),
            bootstrap: CONFIGURATION + 0x80,
            allocate: p,
            release: p + 4,
            cache: vec![p + 8, p + 12, p + 16, p + 20],
            stock_reader: flash
                .code_ranges
                .iter()
                .find(|r| r.name == "nand_read_transfer")
                .unwrap()
                .address as u32,
            body: good_image()[32..].to_vec(),
            heap: HEAP,
            ..Exr2::default()
        });
    }
    c
}

struct FlashRun {
    capture: Option<Result<Manifest>>,
    flash: Result<FlashRecord>,
    sunk: Vec<(u64, Vec<u8>)>,
}
/// State capture (sets the RAM gates), then the flash phase under its policy.
fn flash_run(
    facts: &ServiceFacts,
    c: &Arc<Mutex<Camera>>,
    flash: &FirmwareFlash,
    reader: Option<&Reader>,
    with_capture: bool,
) -> FlashRun {
    let (bulk, state) = (bulk(), state());
    let inputs = Inputs {
        facts,
        bulk: &bulk,
        state: &state,
        flash: Some(flash),
        reader,
    };
    let mut sunk = Vec::new();
    let (capture, flash) = block_on(async {
        let mut session = peer_session(Device(c.clone()), 2 << 20, None)
            .await
            .unwrap();
        let capture = if with_capture {
            Some(
                inputs
                    .capture(
                        &mut session,
                        USB,
                        &mut |n, _| Ok(format!("{n}.bin")),
                        &mut |_, _| {},
                    )
                    .await,
            )
        } else {
            None
        };
        c.lock().unwrap().policy = Some(Box::new(inputs.flash_policy()));
        let flash = inputs
            .back_up_flash(
                &mut session,
                USB,
                &mut |offset, bytes| {
                    sunk.push((offset, bytes.to_vec()));
                    Ok(())
                },
                &mut |_, _| {},
            )
            .await;
        (capture, flash)
    });
    FlashRun {
        capture,
        flash,
        sunk,
    }
}

#[test]
fn flash_backup_uploads_runs_reads_twice_and_cleans_up() {
    let flash = test_flash();
    let image = good_image();
    let (pin, _) = reader_parts(&image, &flash);
    let facts = flash_facts(&pin);
    let reader = verified(&facts, &flash, &image).expect("reader verifies");
    let c = flash_camera(&facts, &flash);
    let original_bootstrap = c.lock().unwrap().bytes(CONFIGURATION + 0x80, 32);
    let run = flash_run(&facts, &c, &flash, Some(&reader), true);
    run.capture.unwrap().expect("state capture");
    let record = run.flash.expect("flash backup");
    let camera = c.lock().unwrap();
    assert!(camera.violations.is_empty(), "{:?}", camera.violations);
    // Read twice, hashed, and every byte delivered once in the first read.
    let data = nand_bytes();
    assert_eq!(record.reads, 2);
    assert_eq!(record.size, FLASH_BYTES);
    assert_eq!(record.sha256, hex(&checks::sha256_bytes(&data)));
    let mut assembled = vec![0; FLASH_BYTES as usize];
    for (offset, bytes) in &run.sunk {
        assembled[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
    }
    assert_eq!(assembled, data);
    assert_eq!(record.upload.image_sha256, pin.image_sha256);
    assert_eq!(
        record.upload.bootstrap_address,
        u64::from(CONFIGURATION) + 0x80
    );
    // Upload, run, cleanup: one block per read, each freed; the reader ran
    // only the stock page reader; nothing else executed.
    let e = camera.exr2.as_ref().unwrap();
    assert_eq!(e.allocations.len(), 2);
    assert_eq!(e.released, e.allocations);
    assert!(e.reader_runs >= 4, "control + data per read");
    assert!(e.stock_entries.iter().all(|&s| s == e.stock_reader));
    let allowed: Vec<u32> = [e.bootstrap]
        .into_iter()
        .chain(e.cache.iter().copied())
        .chain(e.allocations.iter().copied())
        .collect();
    assert!(e.executed.iter().all(|x| allowed.contains(x)));
    // The upload area is back as it was.
    assert_eq!(
        camera.word_at(camera.callback_slot()),
        camera.stock_callback
    );
    assert_eq!(camera.bytes(CONFIGURATION + 0x80, 32), original_bootstrap);
    assert_eq!(record.cleanup.len(), 2);
    for cleanup in &record.cleanup {
        assert!(cleanup.callback_restored && cleanup.bootstrap_restored);
        assert!(cleanup.source_state_restored && cleanup.ram_gates_unchanged);
        assert!(cleanup.allocation_released);
    }
    // The flash phase wrote no configuration byte: only the capture's two gates.
    assert_eq!(
        camera.configuration_writes,
        [(EXR2_READ_GATE, 1), (EXR2_RESET_GUARD, 1)]
    );
}

#[test]
fn missing_facts_still_refuse_the_flash_backup_before_any_usb_command() {
    let flash = test_flash();
    let image = good_image();
    let (pin, _) = reader_parts(&image, &flash);
    let pinned = flash_facts(&pin);
    let reader = verified(&pinned, &flash, &image).unwrap();
    // Bundled facts: read_gate, reset_guard, slow_read_bytes and the reader
    // are unlocated, even with a verified reader handed in.
    let mut bundled = ServiceFacts::bundled();
    bundled.flash_reader = high(pin.clone());
    let mut cases = vec![(bundled, Some(&reader), "read_gate")];
    // Gates located, but no pinned reader.
    let mut unpinned = pinned.clone();
    unpinned.flash_reader = ServiceFacts::bundled().flash_reader;
    cases.push((unpinned, None, "flash_reader"));
    // Gates located at offsets the reused reader does not read.
    let mut elsewhere = located();
    elsewhere.flash_reader = high(pin.clone());
    cases.push((elsewhere, Some(&reader), "read_gate"));
    for (facts, reader, first) in cases {
        let c = flash_camera(&pinned, &flash);
        let run = flash_run(&facts, &c, &flash, reader, false);
        let Err(Error::Missing(missing)) = run.flash else {
            panic!("flash must refuse: {:?}", run.flash.err());
        };
        assert!(missing.iter().any(|m| m.fact == first), "{missing:?}");
        assert!(c.lock().unwrap().commands.is_empty(), "no USB command");
    }
    // And the state capture still refuses with the bundled facts.
    assert!(matches!(
        capture(&ServiceFacts::bundled(), &camera(Policy::read_only()), USB).result,
        Err(Error::Missing(_))
    ));
    // Nothing is embedded at build time while the facts pin no reader.
    assert!(ServiceFacts::bundled().flash_reader.value.is_none());
    assert!(crate::flash::BUNDLED_READER.is_none());
}

#[test]
fn modified_reader_image_or_artifact_is_rejected() {
    let flash = test_flash();
    let image = good_image();
    let (pin, artifact) = reader_parts(&image, &flash);
    let facts = flash_facts(&pin);
    let bulk = bulk();
    Reader::verify(&pin, &artifact, &image, &facts, &flash, &bulk).unwrap();
    let mut changed = image.clone();
    changed[36] ^= 1;
    let error = Reader::verify(&pin, &artifact, &changed, &facts, &flash, &bulk).unwrap_err();
    assert!(error.to_string().contains("pinned sha256"), "{error}");
    let mut changed_artifact = artifact.clone();
    let last = changed_artifact.len() - 2;
    changed_artifact[last] = b' ';
    assert!(Reader::verify(&pin, &changed_artifact, &image, &facts, &flash, &bulk).is_err());
    // A camera block holding a different reader never runs.
    let c = flash_camera(&facts, &flash);
    c.lock().unwrap().exr2.as_mut().unwrap().body[4] ^= 1;
    let reader = Reader::verify(&pin, &artifact, &image, &facts, &flash, &bulk).unwrap();
    let run = flash_run(&facts, &c, &flash, Some(&reader), true);
    assert!(run.flash.is_err());
    assert!(c.lock().unwrap().exr2.as_ref().unwrap().reader_runs == 0);
}

#[test]
fn reader_images_with_a_possible_flash_write_path_are_rejected_even_when_pinned() {
    let flash = test_flash();
    let cases: [(&str, u32); 8] = [
        ("SVC", 0xef00_0000),
        ("BL into firmware", 0xeb10_0000),
        ("B out of the reader", 0xea00_0400),
        ("MCR (cache/MPU)", 0xee07_0f15),
        ("NAND controller literal", 0xfff5_2000),
        ("DMA controller literal", 0xfff0_2000),
        ("MOVT r0, #0xfff5", 0xe34f_0ff5),
        ("MSR cpsr_c", 0xe321_f0d3),
    ];
    for (name, bad) in cases {
        let image = image_with(&[NOP, bad, BRANCH_SELF]);
        assert!(reader_scan::scan(&image).is_err(), "{name}");
        let (pin, artifact) = reader_parts(&image, &flash);
        let facts = flash_facts(&pin);
        let error = Reader::verify(&pin, &artifact, &image, &facts, &flash, &bulk())
            .expect_err(name)
            .to_string();
        assert!(error.contains("static screen"), "{name}: {error}");
    }
    assert!(reader_scan::scan(&good_image()).is_ok());
}

#[test]
fn no_flash_write_path_is_reachable_in_the_flash_phase() {
    let flash = test_flash();
    let image = good_image();
    let (pin, _) = reader_parts(&image, &flash);
    let facts = flash_facts(&pin);
    let reader = verified(&facts, &flash, &image).unwrap();
    let (bulk, state) = (bulk(), state());
    let inputs = Inputs {
        facts: &facts,
        bulk: &bulk,
        state: &state,
        flash: Some(&flash),
        reader: Some(&reader),
    };
    let p = inputs.flash_policy();
    let write = |address: u32, data: &[u8]| {
        command(policy::MEMORY, 3, [0, 0, address, data.len() as u32], data)
    };
    // Firmware update, upload, console, restart and event commands.
    for code in [0x660002, 0x550001, 0x760001, 0x110000, 0x2a0001] {
        assert!(
            p.check(&command(code, 0, [0; 4], &[])).is_err(),
            "{code:#x}"
        );
    }
    // No configuration write at all, stored or RAM.
    for view in [0, 1] {
        let w = command(policy::CONFIGURATION, 3, [view, 0, EXR2_READ_GATE, 1], &[1]);
        assert!(p.check(&w).is_err());
    }
    // RAM writes: never into the NAND/DMA controllers, the boot ROM or any
    // pinned stock code (including the stock NAND reader itself).
    assert!(
        p.check(&write(0xfff5_2000, &[0x80])).is_err(),
        "NAND controller"
    );
    assert!(
        p.check(&write(0xfff0_2000, &[1])).is_err(),
        "DMA controller"
    );
    assert!(p.check(&write(0x8000_0000, &[1])).is_err(), "boot ROM");
    let stock_reader = flash.code_ranges[0].address as u32;
    assert!(p.check(&write(stock_reader + 8, &[0])).is_err());
    assert!(p.check(&write(stock_reader | ALIAS, &[0])).is_err());
    assert!(p.check(&write(PRIMITIVES as u32, &[0])).is_err());
    assert!(
        p.check(&write(HEAP, &[0, 0])).is_err(),
        "longer than one byte"
    );
    assert!(matches!(
        p.check(&write(HEAP, &[0])),
        Ok(Kind::RamWrite { .. })
    ));
    assert!(matches!(
        p.check(&write(HEAP | ALIAS, &[0])),
        Ok(Kind::RamWrite { .. })
    ));
    // E0 data uploads stay blocked; E1 reads stay allowed.
    assert!(check_control(RequestKind::WriteData).is_err());
    // A catalog contract that names a write-side routine is refused.
    let mut writer = flash.clone();
    writer.code_ranges[2].name = "NAND program page".into();
    assert!(crate::flash::read_side_only(&writer).is_err());
    let mut eraser = flash.clone();
    eraser.reader.as_mut().unwrap().symbol = "nand_erase_block".into();
    assert!(crate::flash::read_side_only(&eraser).is_err());
    assert!(crate::flash::read_side_only(&flash).is_ok());
    // The flash module reaches the reused path only through backup().
    let source = include_str!("flash.rs");
    assert_eq!(
        source.matches("fujimod_workflows::flash::backup(").count(),
        1
    );
    for name in [
        "read_spare",
        "diagnose_nand",
        "read_range(session, &",
        "flash::build",
        "write_range",
        "write_memory",
        "WriteConfiguration",
        "write_byte_verified",
        "research_command",
        "install::",
        "restart::",
        "debug_mode::",
        "service_mode::",
        "send_event",
        "submit_console",
    ] {
        assert!(!source.contains(name), "flash.rs mentions {name}");
    }
}

#[test]
fn failed_cleanup_is_reported_with_what_was_left() {
    let flash = test_flash();
    let image = good_image();
    let (pin, _) = reader_parts(&image, &flash);
    let facts = flash_facts(&pin);
    let reader = verified(&facts, &flash, &image).unwrap();
    let c = flash_camera(&facts, &flash);
    c.lock().unwrap().exr2.as_mut().unwrap().refuse_release = true;
    let run = flash_run(&facts, &c, &flash, Some(&reader), true);
    let Err(Error::Flash(message)) = run.flash else {
        panic!("flash must fail: {:?}", run.flash.map(|_| ()));
    };
    assert!(message.contains("NOT restored"), "{message}");
    assert!(message.contains("bootstrap slot"), "{message}");
    assert!(message.contains("heap allocation"), "{message}");
    assert!(message.contains("pull the battery"), "{message}");
}

#[test]
fn dry_run_and_plan_list_the_upload_and_send_nothing_for_it() {
    let flash = test_flash();
    let image = good_image();
    let (pin, _) = reader_parts(&image, &flash);
    let facts = flash_facts(&pin);
    let reader = verified(&facts, &flash, &image).unwrap();
    let (bulk, state) = (bulk(), state());
    let inputs = Inputs {
        facts: &facts,
        bulk: &bulk,
        state: &state,
        flash: Some(&flash),
        reader: Some(&reader),
    };
    let c = flash_camera(&facts, &flash);
    c.lock().unwrap().policy = Some(Box::new(Policy::read_only()));
    let report = block_on(async {
        let mut session = peer_session(Device(c.clone()), 2 << 20, None)
            .await
            .unwrap();
        inputs.dry_run(&mut session, USB).await
    })
    .expect("dry run");
    let step = report
        .plan
        .iter()
        .find(|s| s.name.starts_with("back up flash"))
        .unwrap();
    assert!(step.ready, "{:?}", step.refused_because);
    let text = step.commands.join("\n");
    assert!(text.contains(&pin.image_sha256), "{text}");
    assert!(
        text.contains(&format!("{:#x}", u64::from(CONFIGURATION) + 0x80)),
        "{text}"
    );
    assert!(
        text.contains(&format!("allocate {:#x}", PRIMITIVES)),
        "{text}"
    );
    assert!(
        text.contains(&format!("{} bytes", image.len() - 32)),
        "{text}"
    );
    assert!(text.contains("twice"), "{text}");
    let camera = c.lock().unwrap();
    assert!(camera.violations.is_empty(), "{:?}", camera.violations);
    assert!(camera.configuration_writes.is_empty());
    let e = camera.exr2.as_ref().unwrap();
    assert!(e.allocations.is_empty() && e.executed.is_empty());
    // And without a pinned reader the offline plan refuses the upload.
    let unpinned = Inputs {
        reader: None,
        ..inputs
    };
    let plan = unpinned.plan();
    let step = plan
        .iter()
        .find(|s| s.name.starts_with("back up flash"))
        .unwrap();
    assert!(!step.ready);
}

#[test]
fn reader_screen_covers_every_exr2_controller_block() {
    let soc: serde_json::Value =
        serde_json::from_str(include_str!("../../../catalog/soc/exr2/soc.json")).unwrap();
    for block in soc["blocks"].as_array().unwrap() {
        let base = block["base"].as_u64().unwrap();
        let end = base + block["size"].as_u64().unwrap();
        assert!(
            reader_scan::PERIPHERAL_SPACE
                .iter()
                .any(|&(s, e)| base >= u64::from(s) && end <= u64::from(e)),
            "{} at {base:#x}",
            block["id"]
        );
    }
}
