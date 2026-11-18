//! Hand-authored service-capture facts with per-fact evidence.
use serde::{Deserialize, Serialize};

/// The checked-in X-E2 4.11 facts. Hand-authored, never generated.
pub const XE2_411: &str = include_str!("../facts/xe2-411-service-facts.json");
pub const FIRMWARE_ID: &str = "xe2-411";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
    Unlocated,
}

/// One fact. It is usable only when `value` is present and `confidence` is high.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fact<T> {
    pub value: Option<T>,
    pub confidence: Confidence,
    pub evidence: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsbIds {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub physical_keys: Vec<String>,
    pub usb_condition: String,
}

/// A byte in configuration RAM (view 1, page 0) and the value that enables it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RamByte {
    pub offset: u32,
    pub enabled_value: u8,
}

/// A located read-only flash reader. None exists for xe2-411; the type is kept
/// so a future fact must state explicitly that its path performs no writes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlashReader {
    pub mechanism: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceFacts {
    pub format: u32,
    pub about: String,
    pub firmware_id: String,
    pub firmware_sha256: String,
    pub service_usb: Fact<UsbIds>,
    pub product: Fact<String>,
    pub internal_version: Fact<String>,
    pub entry: Fact<Entry>,
    /// Always `catalog:bulk.code_checks`: the regions come from the catalog.
    pub code_checks: Fact<String>,
    pub read_gate: Fact<RamByte>,
    pub reset_guard: Fact<RamByte>,
    pub slow_read_bytes: Fact<u32>,
    pub flash_reader: Fact<FlashReader>,
}

/// A fact the tool needs but cannot use, with the recorded reason.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Missing {
    pub fact: &'static str,
    pub evidence: String,
}

impl<T: Clone> Fact<T> {
    /// The value, only when it is present and of high confidence.
    pub fn usable(&self, name: &'static str) -> Result<T, Missing> {
        match (&self.value, self.confidence) {
            (Some(value), Confidence::High) => Ok(value.clone()),
            _ => Err(Missing {
                fact: name,
                evidence: self.evidence.clone(),
            }),
        }
    }
}

impl ServiceFacts {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let facts: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if facts.format != 1 {
            return Err(format!("unsupported facts format {}", facts.format));
        }
        if facts.firmware_id != FIRMWARE_ID {
            return Err(format!(
                "facts are for {}, this tool supports only {FIRMWARE_ID}",
                facts.firmware_id
            ));
        }
        if facts.code_checks.value.as_deref() != Some("catalog:bulk.code_checks") {
            return Err("code_checks must name catalog:bulk.code_checks".into());
        }
        if let (Some(gate), Some(guard)) = (&facts.read_gate.value, &facts.reset_guard.value)
            && gate.offset == guard.offset
        {
            return Err("read gate and reset guard must be different bytes".into());
        }
        for gate in [&facts.read_gate.value, &facts.reset_guard.value]
            .into_iter()
            .flatten()
        {
            if gate.enabled_value == 0 {
                return Err("an enabled RAM gate value must be nonzero".into());
            }
        }
        if facts
            .slow_read_bytes
            .value
            .is_some_and(|n| n == 0 || n > 4096)
        {
            return Err("slow_read_bytes must be 1..=4096".into());
        }
        Ok(facts)
    }
    pub fn bundled() -> Self {
        Self::from_json(XE2_411).expect("checked-in facts parse")
    }
}
