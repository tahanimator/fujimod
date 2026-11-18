//! Safety-gate tests against a stateful mock FF80 camera. The mock applies the
//! write policy to every command and control request it receives.
use super::*;
use crate::{
    facts::{Confidence, Fact},
    policy::{Auditor, Kind, Policy, check_control},
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
    policy: Policy,
    violations: Vec<String>,
    configuration_writes: Vec<(u32, u8)>,
    commands: Vec<u32>,
    base: u32,
    source_state: u32,
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
        match c.policy.check(raw) {
            Err(v) => {
                c.violations.push(v.to_string());
                c.reply = raw[..raw.len().min(36)].to_vec();
                return Ok(());
            }
            Ok(Kind::RamGateWrite { offset, value }) => {
                c.configuration_writes.push((offset, value));
            }
            Ok(Kind::Read) => {}
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
        policy,
        base: base as u32,
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
    }
    .plan();
    let flash = plan.iter().find(|s| s.name == "back up flash").unwrap();
    assert!(!flash.ready);
    assert_eq!(flash.refused_because[0].fact, "flash_reader");
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
        "flash::",
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
