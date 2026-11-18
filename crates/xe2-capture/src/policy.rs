//! The tool's write policy as a pure check of FF80 command bytes.
//!
//! Every command the tool can send is classified here. The test peer applies
//! this check to every command on the wire, and the CLI audits its USB trace
//! with it after each run. Anything not listed is a violation, including every
//! memory write, every storage-view configuration write, data uploads and
//! console, event, restart and firmware-update commands.
//!
//! [`FlashPolicy`] is the separate, narrower allowance for the owner-approved
//! flash backup phase: RAM writes inside DRAM that never touch pinned stock
//! code, and diagnostic queries that run the uploaded reader. It permits no
//! configuration write, no stored write, no data upload (E0) and no firmware,
//! restart or console command, so no flash erase/program request can be sent.
use crate::facts::RamByte;

pub const DEVICE_INFO: u32 = 0x150000;
pub const CONFIGURATION: u32 = 0x1a0002;
pub const MEMORY: u32 = 0x200001;
pub const DIAGNOSTIC_QUERY: u32 = 0x450006;
/// The stock diagnostic selector that points E1 bulk reads at DRAM.
pub const BULK_SOURCE_TAG: u32 = 0x80;

const CONFIGURATION_GEOMETRY: u32 = 1;
const READ: u32 = 2;
const WRITE: u32 = 3;
const VIEW_RAM: u32 = 1;

/// Writes this run may make: only the two configuration-RAM gate bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    /// The debug read gate; it may be enabled, or restored to zero after a
    /// failed code check.
    pub read_gate: Option<RamByte>,
    /// The reset guard; it may only be armed, never cleared.
    pub reset_guard: Option<RamByte>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("blocked FF80 command {command:#x}: {reason}")]
pub struct Violation {
    pub command: u32,
    pub reason: &'static str,
}

/// What a permitted command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Read,
    RamGateWrite {
        offset: u32,
        value: u8,
    },
    /// Flash phase only: a DRAM write outside every pinned code range.
    RamWrite {
        address: u32,
        length: u32,
    },
    /// Flash phase only: a diagnostic query that runs the published callback.
    Execute {
        argument: u32,
    },
}

/// A classifier of FF80 command bytes, applied on the wire and in the audit.
pub trait CommandCheck: Send {
    fn check(&self, request: &[u8]) -> Result<Kind, Violation>;
}
impl CommandCheck for Policy {
    fn check(&self, request: &[u8]) -> Result<Kind, Violation> {
        Policy::check(self, request)
    }
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

impl Policy {
    /// A policy that permits no write at all (the dry run).
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn check(&self, request: &[u8]) -> Result<Kind, Violation> {
        let command = word(request, 0).unwrap_or(0);
        let deny = |reason| Err(Violation { command, reason });
        let Some(function) = word(request, 16) else {
            return deny("short command");
        };
        match command {
            DEVICE_INFO if request.len() == 32 => Ok(Kind::Read),
            MEMORY if function == READ && request.len() == 36 => Ok(Kind::Read),
            MEMORY => deny("debug memory writes are never sent"),
            CONFIGURATION if function == CONFIGURATION_GEOMETRY && request.len() == 36 => {
                Ok(Kind::Read)
            }
            CONFIGURATION if function == READ && request.len() == 36 => Ok(Kind::Read),
            CONFIGURATION if function == WRITE => self.gate_write(command, request),
            CONFIGURATION => deny("unknown configuration function"),
            DIAGNOSTIC_QUERY if function == BULK_SOURCE_TAG && request.len() == 32 => {
                Ok(Kind::Read)
            }
            DIAGNOSTIC_QUERY => deny("only the stock bulk-source query is sent"),
            _ => deny("command is outside the read-only allowlist"),
        }
    }

    fn gate_write(&self, command: u32, request: &[u8]) -> Result<Kind, Violation> {
        let deny = |reason| Err(Violation { command, reason });
        let (Some(view), Some(page), Some(offset), Some(length)) = (
            word(request, 20),
            word(request, 24),
            word(request, 28),
            word(request, 32),
        ) else {
            return deny("short configuration write");
        };
        if view != VIEW_RAM {
            return deny("stored (persistent) configuration writes are never sent");
        }
        if page != 0 || length != 1 || request.len() != 37 {
            return deny("only single-byte page-0 RAM gate writes are allowed");
        }
        let value = request[36];
        let allowed = match (self.read_gate, self.reset_guard) {
            (Some(gate), _) if gate.offset == offset => value == gate.enabled_value || value == 0,
            (_, Some(guard)) if guard.offset == offset => value == guard.enabled_value,
            _ => return deny("configuration RAM byte is not a located gate"),
        };
        if allowed {
            Ok(Kind::RamGateWrite { offset, value })
        } else {
            deny("value is not the gate's enabled or restored value")
        }
    }
}

/// The flash-backup phase allowance. Reads are as in [`Policy::read_only`];
/// configuration writes of any kind are refused (the gates must already be
/// set by the state capture); memory writes must lie wholly in DRAM and miss
/// every protected range; diagnostic queries may carry any argument because
/// the stock dispatcher passes it to the callback the reader path published.
#[derive(Clone, Debug)]
pub struct FlashPolicy {
    /// Physical DRAM `[start, end)`.
    pub dram: (u64, u64),
    /// The uncached alias bit added to physical DRAM addresses.
    pub alias: u64,
    /// Physical `[start, end)` ranges never written: all pinned stock code.
    pub protected: Vec<(u64, u64)>,
    /// Largest memory write per command.
    pub max_write: u32,
}
impl FlashPolicy {
    fn physical(&self, address: u64) -> Option<u64> {
        let (start, end) = self.dram;
        let address = if address >= self.alias && address - self.alias < end {
            address - self.alias
        } else {
            address
        };
        (address >= start && address < end).then_some(address)
    }
    pub fn check(&self, request: &[u8]) -> Result<Kind, Violation> {
        let command = word(request, 0).unwrap_or(0);
        let deny = |reason| Err(Violation { command, reason });
        let Some(function) = word(request, 16) else {
            return deny("short command");
        };
        match command {
            MEMORY if function == WRITE => {
                let (Some(address), Some(length)) = (word(request, 28), word(request, 32)) else {
                    return deny("short memory write");
                };
                if length == 0 || length > self.max_write || request.len() != 36 + length as usize {
                    return deny("memory write length differs from its data");
                }
                let first = self.physical(u64::from(address));
                let last = self.physical(u64::from(address) + u64::from(length) - 1);
                let (Some(first), Some(last)) = (first, last) else {
                    return deny("memory writes outside DRAM are never sent");
                };
                if last < first || last - first + 1 != u64::from(length) {
                    return deny("memory write crosses the DRAM alias");
                }
                if self
                    .protected
                    .iter()
                    .any(|&(start, end)| first < end && start <= last)
                {
                    return deny("memory write overlaps pinned stock code");
                }
                Ok(Kind::RamWrite { address, length })
            }
            DIAGNOSTIC_QUERY if request.len() == 32 => Ok(Kind::Execute { argument: function }),
            CONFIGURATION if function == WRITE => {
                deny("no configuration write is sent during the flash backup")
            }
            _ => Policy::read_only().check(request),
        }
    }
}
impl CommandCheck for FlashPolicy {
    fn check(&self, request: &[u8]) -> Result<Kind, Violation> {
        FlashPolicy::check(self, request)
    }
}

/// Data-phase control requests: E1 reads are allowed, E0 data uploads are not.
pub fn check_control(kind: fujimod_usb::ff80::framing::RequestKind) -> Result<(), Violation> {
    use fujimod_usb::ff80::framing::RequestKind as K;
    match kind {
        K::WriteData => Err(Violation {
            command: kind as u32,
            reason: "data uploads (E0) are never sent",
        }),
        K::EmptyResponse => Err(Violation {
            command: kind as u32,
            reason: "not used by this tool",
        }),
        _ => Ok(()),
    }
}

/// Session lifecycle envelopes (open, ping, close, reclaim) carry no command.
pub fn check_lifecycle(envelope: &[u8]) -> Result<(), Violation> {
    use fujimod_usb::ff80::framing::RequestKind as K;
    let kind = envelope.first().copied().unwrap_or(0);
    let allowed = [K::Open, K::Ping, K::Close, K::Reclaim].map(|k| k as u8);
    if envelope.len() == 12 && allowed.contains(&kind) {
        Ok(())
    } else {
        Err(Violation {
            command: kind.into(),
            reason: "unknown session envelope",
        })
    }
}

/// Incremental audit of a USB trace against a policy. Drain it often during
/// long transfers so the bounded trace never drops events.
pub struct Auditor {
    pub policy: Box<dyn CommandCheck>,
    pub commands: usize,
    pub first_violation: Option<String>,
}
impl Auditor {
    pub fn new(policy: impl CommandCheck + 'static) -> Self {
        Self {
            policy: Box::new(policy),
            commands: 0,
            first_violation: None,
        }
    }
    pub fn absorb(&mut self, batch: fujimod_usb::trace::Batch) {
        use fujimod_usb::{
            error::TransferStage,
            trace::{Action, Layer},
        };
        let mut violation = |message: String| {
            self.first_violation.get_or_insert(message);
        };
        if batch.dropped_events != 0 {
            violation("USB trace dropped events; the command audit is incomplete".into());
        }
        for event in batch.events {
            if let (TransferStage::Control(kind), Action::Submitted { .. }) =
                (event.stage, &event.action)
                && let Err(v) = check_control(kind)
            {
                violation(v.to_string());
            }
        }
        for event in batch.protocol_events {
            let Action::Submitted { data, .. } = &event.action else {
                continue;
            };
            if event.layer != Layer::Ff80 {
                violation("non-FF80 protocol traffic".into());
                continue;
            }
            let checked = if data.len() == 12 {
                check_lifecycle(data)
            } else {
                self.policy.check(data).map(|_| ())
            };
            match checked {
                Ok(()) => self.commands += 1,
                Err(v) => violation(v.to_string()),
            }
        }
    }
    /// Switch to another phase's allowance after draining the trace.
    pub fn switch(&mut self, policy: impl CommandCheck + 'static) {
        self.policy = Box::new(policy);
    }
    pub fn finish(self) -> Result<usize, String> {
        match self.first_violation {
            Some(v) => Err(v),
            None => Ok(self.commands),
        }
    }
}
