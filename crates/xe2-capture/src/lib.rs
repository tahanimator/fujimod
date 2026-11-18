//! Local X-E2 4.11 service-mode capture: identity, a dry run, a camera state
//! dump (DRAM and boot ROM) through the stock FF80 readers, and an
//! owner-approved whole-flash backup through temporary code in camera RAM.
//!
//! Safety model, enforced here and checked by [`policy`]:
//! - Flash is never erased, programmed or written. The flash backup in
//!   [`flash`] reuses the existing EXR II reader path unchanged with a reviewed
//!   reader image pinned by hash, screened statically, uploaded to a heap block
//!   in RAM and cleaned up afterwards; its own phase policy admits no flash,
//!   firmware, stored-setting or configuration write.
//! - Nothing persists: no stored-configuration writes, debug-mode save,
//!   restart, firmware update, event or console command.
//! - The state capture's only writes are the configuration-RAM read gate and
//!   reset guard, each a single verified byte whose original value is recorded
//!   first. Both need high-confidence located facts, and each step refuses
//!   without them; the flash step needs the same facts plus a pinned reader.
//! - Identity (USB IDs, product, internal version) is verified before the
//!   first write; stock code is verified before the reset guard, before any
//!   capture and before any upload. The code checks run first with the read
//!   gate as found; only if the camera refuses those reads is the read gate
//!   enabled before them, and then a failed check restores it.
#![forbid(unsafe_code)]

pub mod facts;
pub mod flash;
pub mod policy;
pub mod reader_scan;

use facts::{Missing, RamByte, ServiceFacts, UsbIds};
use fujimod_catalog::capture::{FirmwareFlash, FirmwareState};
use fujimod_catalog_bulk::{BulkSource, FirmwareBulk};
use fujimod_usb::{
    ff80::{
        Session, SessionState,
        commands::{
            configuration::ConfigurationView,
            info::InfoSelector,
            memory::{Address, DebugMemoryLimits, MemoryRange},
        },
    },
    operations::{checks, configuration},
};
use serde::Serialize;
use std::num::NonZeroU32;

pub const BATTERY_REMINDER: &str = "Exit service mode by pulling the battery. Never use the OFF switch: \
     a normal shutdown in service mode can restore factory defaults.";

/// The loud, multi-line warning printed before anything else when the
/// `--assume-x70-values` override is in force. It names what is borrowed, the
/// physical risk, the recovery, and that this is the owner's informed choice.
pub const X70_ASSUMED_WARNING: &str = "\
====================================================================
  WARNING: RUNNING WITH UNVERIFIED, ASSUMED X70 SERVICE VALUES
====================================================================
  read_gate, reset_guard and slow_read_bytes are NOT located on the
  X-E2. They are borrowed UNCHANGED from the X70 1.10 firmware, on the
  assumption that the shared EXR II processor uses the same values.
  They have NOT been confirmed on the X-E2.

  If a borrowed RAM gate or guard ADDRESS is wrong for the real X-E2,
  writing it can CRASH, HANG or RESET the camera, or reset it to
  factory defaults. This is a real risk, not a theoretical one.

  If the camera misbehaves, stops responding, or reboots: PULL THE
  BATTERY to power it down. Do NOT use the OFF switch.

  Proceeding is the owner's informed choice. Any dump produced this way
  is labelled in its manifest as captured under assumed X70 values.
====================================================================";

/// What the dry run can and cannot tell you about the assumed values.
pub const X70_ASSUMED_DRY_RUN_NOTE: &str =
    "A dry run sends nothing to the camera, so it CANNOT confirm that the assumed X70 values are \
     correct for the X-E2. Only a real run reveals that. Treat the first real capture as the \
     actual test of these values, and be ready to pull the battery.";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Usb(#[from] fujimod_usb::Error),
    #[error("refused before any write: {0}")]
    Identity(String),
    #[error("refused: missing or low-confidence facts: {}", names(.0))]
    Missing(Vec<Missing>),
    #[error("refused: {0}")]
    Refused(String),
    #[error("stock code differs from xe2-411: {0}")]
    CodeDiffers(String),
    #[error("{0}")]
    Capture(String),
    #[error("flash backup stopped: {0}")]
    Flash(String),
    #[error("flash backup cleanup incomplete: {0}")]
    Cleanup(String),
}
fn names(missing: &[Missing]) -> String {
    missing
        .iter()
        .map(|m| m.fact)
        .collect::<Vec<_>>()
        .join(", ")
}
pub type Result<T> = std::result::Result<T, Error>;

/// Catalog facts the tool reads, plus the hand-authored service facts.
pub struct Inputs<'a> {
    pub facts: &'a ServiceFacts,
    pub bulk: &'a FirmwareBulk,
    pub state: &'a FirmwareState,
    pub flash: Option<&'a FirmwareFlash>,
    /// The verified, pinned flash reader, if the facts pin one.
    pub reader: Option<&'a flash::Reader>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Step {
    pub name: &'static str,
    pub ready: bool,
    pub refused_because: Vec<Missing>,
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    pub usb: UsbIds,
    pub product: String,
    pub internal_version: String,
    pub serial: Option<String>,
    pub configuration_ram_bytes: u32,
    pub configuration_page_bytes: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct GateRecord {
    pub name: &'static str,
    pub offset: u32,
    pub original: u8,
    pub set_to: u8,
    pub written: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RegionRecord {
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub constant: bool,
    /// Constant regions are read twice and must match.
    pub reads: u32,
    pub sha256: Option<String>,
    pub file: Option<String>,
    /// All-zero or all-0xff constant regions are masked, not saved.
    pub masked: bool,
}

/// One located fact that was supplied by the X70 override rather than located
/// on the X-E2, recorded in the manifest so any dump is clearly labelled as
/// captured under unverified parameters.
#[derive(Clone, Debug, Serialize)]
pub struct AssumedFact {
    pub fact: &'static str,
    pub provenance: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Manifest {
    pub tool: &'static str,
    pub firmware_id: String,
    pub firmware_sha256: String,
    /// Non-empty when the run used `--assume-x70-values`: the three borrowed
    /// facts and their provenance. A dump with this field was captured under
    /// unverified X70 values, not confirmed X-E2 facts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assumed_x70_values: Vec<AssumedFact>,
    pub identity: Identity,
    pub gates: Vec<GateRecord>,
    pub code_checks: Vec<String>,
    /// True when the camera allowed the stock code checks with the read gate
    /// closed, so they passed before any write.
    pub code_verified_before_any_write: bool,
    pub regions: Vec<RegionRecord>,
    pub refused: Vec<Step>,
    /// Filled by the CLI after [`Inputs::back_up_flash`] when it was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flash: Option<flash::FlashRecord>,
    pub reminder: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct DryRun {
    pub identity: Identity,
    pub read_gate_current: Option<u8>,
    pub reset_guard_current: Option<u8>,
    pub code_checks: String,
    pub plan: Vec<Step>,
    /// Non-empty when the dry run used `--assume-x70-values`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assumed_x70_values: Vec<AssumedFact>,
    /// Present with the override: the dry run cannot confirm the assumed values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assumed_note: Option<&'static str>,
    pub reminder: &'static str,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Inputs<'_> {
    fn require(&self, wanted: &[&'static str]) -> Vec<Missing> {
        let f = self.facts;
        wanted
            .iter()
            .filter_map(|&name| match name {
                "service_usb" => f.service_usb.usable(name).err(),
                "product" => f.product.usable(name).err(),
                "internal_version" => f.internal_version.usable(name).err(),
                "read_gate" => f.read_gate.usable(name).err(),
                "reset_guard" => f.reset_guard.usable(name).err(),
                "slow_read_bytes" => f.slow_read_bytes.usable(name).err(),
                "flash_reader" => f.flash_reader.usable(name).err(),
                _ => unreachable!("unknown fact {name}"),
            })
            .collect()
    }

    /// Catalog and fact consistency, checked offline before USB is touched.
    pub fn validate(&self) -> Result<()> {
        let f = self.facts;
        let bad = |m: &str| Err(Error::Refused(m.to_string()));
        if self.bulk.firmware_id != f.firmware_id || self.state.firmware_id != f.firmware_id {
            return bad("catalog and facts name different firmware");
        }
        if f.product.value.as_deref() != Some(self.bulk.product.as_str())
            || f.internal_version.value.as_deref() != Some(self.bulk.internal_version.as_str())
        {
            return bad("catalog bulk identity differs from the facts");
        }
        // Only the query source qualifies without memory writes: the pattern
        // sources fill scratch and the borrowed source rewrites state words.
        if !matches!(self.bulk.source, BulkSource::Query { .. }) {
            return bad("bulk source is not the write-free stock query");
        }
        if self.bulk.code_checks.is_empty() {
            return bad("catalog has no stock code checks");
        }
        self.bulk.validate().map_err(Error::Refused)?;
        self.state.validate().map_err(Error::Refused)?;
        Ok(())
    }

    /// The located facts the current run took from the X70 override, for the
    /// manifest and the dry-run report. Empty under the honest facts.
    pub fn assumed_x70(&self) -> Vec<AssumedFact> {
        self.facts
            .assumed_x70()
            .into_iter()
            .map(|(fact, provenance)| AssumedFact { fact, provenance })
            .collect()
    }

    fn policy(&self) -> policy::Policy {
        policy::Policy {
            read_gate: self.facts.read_gate.usable("read_gate").ok(),
            reset_guard: self.facts.reset_guard.usable("reset_guard").ok(),
        }
    }

    fn limits(&self) -> Result<DebugMemoryLimits> {
        let read = self
            .facts
            .slow_read_bytes
            .usable("slow_read_bytes")
            .map_err(|m| Error::Missing(vec![m]))?;
        Ok(DebugMemoryLimits {
            read: NonZeroU32::new(read).ok_or(Error::Refused("zero read size".into()))?,
            // No memory write is ever sent; the policy blocks function 3.
            write: NonZeroU32::MIN,
        })
    }

    /// The exact planned command sequence, with each step's readiness.
    pub fn plan(&self) -> Vec<Step> {
        let f = self.facts;
        let gate = f.read_gate.value;
        let guard = f.reset_guard.value;
        let byte = |b: Option<RamByte>| {
            b.map_or("<unlocated>".into(), |b| {
                format!("offset {:#x} = {:#x}", b.offset, b.enabled_value)
            })
        };
        let usb = f.service_usb.value.map_or("<unlocated>".into(), |u| {
            format!("{:04x}:{:04x}", u.vendor_id, u.product_id)
        });
        let mut steps = vec![
            Step {
                name: "identify",
                ready: true,
                refused_because: self.require(&["service_usb", "product", "internal_version"]),
                commands: vec![
                    format!("USB descriptor must be {usb}"),
                    "FF80 0x150000 info Product -> must equal X-E2".into(),
                    format!(
                        "FF80 0x150000 info InternalVersion -> must equal {}",
                        f.internal_version.value.as_deref().unwrap_or("<unlocated>")
                    ),
                    "FF80 0x150000 info Serial (recorded only)".into(),
                    "FF80 0x1a0002 fn1 RAM configuration geometry (read)".into(),
                    format!("FF80 0x1a0002 fn2 read RAM byte read_gate {}", byte(gate)),
                    format!(
                        "FF80 0x1a0002 fn2 read RAM byte reset_guard {}",
                        byte(guard)
                    ),
                ],
            },
            Step {
                name: "enable read gate (RAM only, not stored)",
                ready: true,
                refused_because: self.require(&["read_gate"]),
                commands: vec![format!(
                    "if 0: FF80 0x1a0002 fn3 write RAM page 0 {}, then read back; else if not \
                     enabled: abort",
                    byte(gate)
                )],
            },
            Step {
                name: "verify stock code",
                ready: true,
                refused_because: self.require(&["read_gate", "slow_read_bytes"]),
                commands: self
                    .bulk
                    .code_checks
                    .iter()
                    .map(|c| {
                        format!(
                            "FF80 0x200001 fn2 read {:#x}+{:#x} ({}) -> sha256 {}",
                            c.address | self.bulk.code_alias,
                            c.size,
                            c.name,
                            c.sha256
                        )
                    })
                    .chain(std::iter::once(
                        "on mismatch: restore read gate to its original value and abort".into(),
                    ))
                    .collect(),
            },
            Step {
                name: "arm reset guard (RAM only, not stored)",
                ready: true,
                refused_because: self.require(&["reset_guard"]),
                commands: vec![format!(
                    "FF80 0x1a0002 fn3 write RAM page 0 {}, then read back",
                    byte(guard)
                )],
            },
        ];
        let mut dump = vec![
            "repeat identity and stock code checks (workflows::state::dump)".to_string(),
            "read the stock diagnostic callback slot and compare".into(),
            format!(
                "FF80 0x450006 diagnostic query tag {:#x} (stock bulk source)",
                policy::BULK_SOURCE_TAG
            ),
            "compare bulk (E1) and slow reads of the catalog probes and mapped probe".into(),
        ];
        for r in &self.state.regions {
            dump.push(format!(
                "read {} {:#x}+{:#x}{}",
                r.name,
                r.address,
                r.size,
                if r.constant {
                    " twice, require equal"
                } else {
                    ""
                }
            ));
        }
        steps.push(Step {
            name: "dump camera state",
            ready: true,
            refused_because: self.require(&[
                "service_usb",
                "product",
                "internal_version",
                "read_gate",
                "reset_guard",
                "slow_read_bytes",
            ]),
            commands: dump,
        });
        steps.push(Step {
            name: "back up flash (temporary reader in RAM)",
            ready: true,
            refused_because: self.flash_refusals(),
            commands: self.flash_plan(),
        });
        for step in &mut steps {
            step.ready = step.refused_because.is_empty();
        }
        steps
    }

    /// Identity checks. Read-only; no command here can change camera state.
    pub async fn identify(&self, session: &mut Session, usb: UsbIds) -> Result<Identity> {
        let f = self.facts;
        let missing = self.require(&["service_usb", "product", "internal_version"]);
        if !missing.is_empty() {
            return Err(Error::Missing(missing));
        }
        if Some(usb) != f.service_usb.value {
            return Err(Error::Identity(format!(
                "USB device {:04x}:{:04x} is not the X-E2 service device",
                usb.vendor_id, usb.product_id
            )));
        }
        let product = session.device_info(InfoSelector::Product).await?.text;
        if Some(product.trim()) != f.product.value.as_deref() {
            return Err(Error::Identity(format!(
                "camera reports product {product:?}, expected X-E2"
            )));
        }
        let version = session
            .device_info(InfoSelector::InternalVersion)
            .await?
            .text;
        if Some(version.trim()) != f.internal_version.value.as_deref() {
            return Err(Error::Identity(format!(
                "camera reports internal version {version:?}, not firmware 4.11"
            )));
        }
        let serial = session
            .device_info(InfoSelector::Serial)
            .await
            .ok()
            .map(|i| i.text.trim().to_string());
        let geometry = session
            .configuration_geometry(ConfigurationView::Ram)
            .await?;
        Ok(Identity {
            usb,
            product: product.trim().into(),
            internal_version: version.trim().into(),
            serial,
            configuration_ram_bytes: geometry.capacity,
            configuration_page_bytes: geometry.page_bytes.get(),
        })
    }

    async fn ram_byte(session: &mut Session, offset: u32) -> Result<u8> {
        Ok(
            configuration::read_byte(session, ConfigurationView::Ram, offset, NonZeroU32::MIN)
                .await?,
        )
    }

    /// The only write path in this crate: one checked configuration-RAM byte.
    async fn set_gate(&self, session: &mut Session, offset: u32, value: u8) -> Result<()> {
        let command = fujimod_usb::ff80::commands::configuration::WriteConfiguration::new(
            fujimod_usb::ff80::commands::configuration::ConfigurationLocation {
                view: ConfigurationView::Ram,
                page: 0,
                offset,
            },
            vec![value],
            NonZeroU32::MIN,
        )?;
        // Check the exact bytes against the policy before anything is sent.
        self.policy()
            .check(&command.encode())
            .map_err(|v| Error::Refused(v.to_string()))?;
        configuration::write_byte_verified(
            session,
            ConfigurationView::Ram,
            offset,
            value,
            NonZeroU32::MIN,
        )
        .await?;
        Ok(())
    }

    async fn verify_code(&self, session: &mut Session, limits: DebugMemoryLimits) -> Result<()> {
        for check in &self.bulk.code_checks {
            let address = u32::try_from(check.address | self.bulk.code_alias)
                .map_err(|_| Error::Refused("code check beyond 32 bits".into()))?;
            let size = u32::try_from(check.size)
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or(Error::Refused("invalid code check size".into()))?;
            let expected: [u8; 32] = (0..32)
                .map(|i| u8::from_str_radix(&check.sha256[2 * i..2 * i + 2], 16))
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or(Error::Refused("invalid code check digest".into()))?;
            let range = MemoryRange::new(Address(address), size)?;
            if !checks::region_matches(session, range, limits, expected).await? {
                return Err(Error::CodeDiffers(check.name.clone()));
            }
        }
        Ok(())
    }

    /// Identify and plan. Sends only reads; code checks run only when the read
    /// gate is already enabled, because enabling it would be a write.
    pub async fn dry_run(&self, session: &mut Session, usb: UsbIds) -> Result<DryRun> {
        self.validate()?;
        let identity = self.identify(session, usb).await?;
        let gate = self.facts.read_gate.usable("read_gate").ok();
        let guard = self.facts.reset_guard.usable("reset_guard").ok();
        let read_gate_current = match gate {
            Some(g) => Some(Self::ram_byte(session, g.offset).await?),
            None => None,
        };
        let reset_guard_current = match guard {
            Some(g) => Some(Self::ram_byte(session, g.offset).await?),
            None => None,
        };
        // Reads only: try the checks with the gate as found.
        let code_checks = match self.limits() {
            Ok(limits) => match self.verify_code(session, limits).await {
                Ok(()) => format!("{} regions match stock", self.bulk.code_checks.len()),
                Err(Error::Usb(fujimod_usb::Error::DeviceStatus { .. })) => {
                    "not run: the camera refuses debug reads while the read gate is closed, and \
                     a dry run never writes; capture checks code before the reset guard and any \
                     capture"
                        .into()
                }
                Err(error) => return Err(error),
            },
            Err(Error::Missing(m)) => format!("not run: missing {}", names(&m)),
            Err(error) => return Err(error),
        };
        let assumed_x70_values = self.assumed_x70();
        let assumed_note = (!assumed_x70_values.is_empty()).then_some(X70_ASSUMED_DRY_RUN_NOTE);
        Ok(DryRun {
            identity,
            read_gate_current,
            reset_guard_current,
            code_checks,
            plan: self.plan(),
            assumed_x70_values,
            assumed_note,
            reminder: BATTERY_REMINDER,
        })
    }

    /// Full capture. `save(name, bytes)` stores one region and returns its
    /// file name. Every step whose facts are missing is refused up front.
    pub async fn capture(
        &self,
        session: &mut Session,
        usb: UsbIds,
        save: &mut dyn FnMut(&str, &[u8]) -> std::result::Result<String, String>,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Manifest> {
        self.validate()?;
        let plan = self.plan();
        let dump = plan
            .iter()
            .find(|s| s.name == "dump camera state")
            .expect("planned dump");
        if !dump.ready {
            return Err(Error::Missing(dump.refused_because.clone()));
        }
        let gate = self.facts.read_gate.usable("read_gate").expect("ready");
        let guard = self.facts.reset_guard.usable("reset_guard").expect("ready");
        let limits = self.limits()?;
        let identity = self.identify(session, usb).await?;
        if gate.offset >= identity.configuration_ram_bytes
            || guard.offset >= identity.configuration_ram_bytes
        {
            return Err(Error::Identity(
                "configuration RAM is smaller than the located gate offsets".into(),
            ));
        }
        let gate_before = Self::ram_byte(session, gate.offset).await?;
        let guard_before = Self::ram_byte(session, guard.offset).await?;
        for (name, value, want) in [
            ("read gate", gate_before, gate.enabled_value),
            ("reset guard", guard_before, guard.enabled_value),
        ] {
            if value != 0 && value != want {
                return Err(Error::Identity(format!(
                    "{name} holds unexpected value {value:#x}"
                )));
            }
        }
        // Verify stock code before any write when the camera allows reads with
        // the gate closed. Only a refused read (device status) leads to the
        // gate write; any other failure, including changed code, aborts here.
        let verified_before_write = match self.verify_code(session, limits).await {
            Ok(()) => true,
            Err(Error::Usb(fujimod_usb::Error::DeviceStatus { .. })) if gate_before == 0 => false,
            Err(error) => return Err(error),
        };
        if gate_before == 0 {
            self.set_gate(session, gate.offset, gate.enabled_value)
                .await?;
        }
        if !verified_before_write && let Err(error) = self.verify_code(session, limits).await {
            if gate_before == 0 && session.state() == SessionState::Ready {
                let _ = self.set_gate(session, gate.offset, 0).await;
            }
            return Err(error);
        }
        if guard_before == 0 {
            self.set_gate(session, guard.offset, guard.enabled_value)
                .await?;
        }
        let gates = vec![
            GateRecord {
                name: "read_gate",
                offset: gate.offset,
                original: gate_before,
                set_to: gate.enabled_value,
                written: gate_before == 0,
            },
            GateRecord {
                name: "reset_guard",
                offset: guard.offset,
                original: guard_before,
                set_to: guard.enabled_value,
                written: guard_before == 0,
            },
        ];
        let mut regions: Vec<RegionRecord> = self
            .state
            .regions
            .iter()
            .map(|r| RegionRecord {
                name: r.name.clone(),
                address: r.address,
                size: r.size,
                constant: r.constant,
                reads: if r.constant { 2 } else { 1 },
                sha256: None,
                file: None,
                masked: true,
            })
            .collect();
        let workflow_limits = fujimod_workflows::Limits {
            memory: limits,
            configuration: NonZeroU32::MIN,
        };
        let mut failure = None;
        let result = fujimod_workflows::state::dump(
            session,
            self.bulk,
            self.state,
            workflow_limits,
            &mut |captured| {
                let sha = hex(&checks::sha256_bytes(&captured.data));
                if sha != captured.sha256 {
                    return Err("region digest differs".into());
                }
                let file = save(&captured.region.name, &captured.data).inspect_err(|e| {
                    failure = Some(e.clone());
                })?;
                let record = regions
                    .iter_mut()
                    .find(|r| r.name == captured.region.name)
                    .ok_or("unplanned region")?;
                record.sha256 = Some(sha);
                record.file = Some(file);
                record.masked = false;
                Ok(())
            },
            &mut |done, total| {
                progress(done, total);
                Ok(())
            },
        )
        .await;
        if let Err(error) = result {
            return Err(Error::Capture(failure.unwrap_or_else(|| error.to_string())));
        }
        for r in &mut regions {
            if !r.constant && r.sha256.is_none() {
                return Err(Error::Capture(format!(
                    "region {} was not delivered",
                    r.name
                )));
            }
        }
        Ok(Manifest {
            tool: concat!("fujimod-xe2-capture ", env!("CARGO_PKG_VERSION")),
            firmware_id: self.facts.firmware_id.clone(),
            firmware_sha256: self.facts.firmware_sha256.clone(),
            assumed_x70_values: self.assumed_x70(),
            identity,
            gates,
            code_checks: self
                .bulk
                .code_checks
                .iter()
                .map(|c| c.name.clone())
                .collect(),
            code_verified_before_any_write: verified_before_write,
            regions,
            refused: plan.into_iter().filter(|s| !s.ready).collect(),
            flash: None,
            reminder: BATTERY_REMINDER,
        })
    }
}

#[cfg(test)]
mod tests;
