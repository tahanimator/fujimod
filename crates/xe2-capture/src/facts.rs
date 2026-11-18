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

/// The reviewed EXR II flash reader pinned for upload to camera RAM. Both
/// files sit beside the facts file and are embedded by `build.rs` only after
/// their hashes match these pins and the image passes the static screen.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlashReader {
    /// Always `exr2-query-heap`, the transport in flash.generated.json.
    pub mechanism: String,
    /// `fujimod_workflows::flash::Artifact` JSON (payload metadata).
    pub artifact: String,
    pub artifact_sha256: String,
    /// The binary image the artifact names (bootstrap + reader + observer).
    pub image: String,
    pub image_sha256: String,
    pub image_bytes: u32,
    /// Who reviewed the reader and from which source revision.
    pub review: String,
}
pub const FLASH_MECHANISM: &str = "exr2-query-heap";

fn sha256_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
fn plain_file(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
impl FlashReader {
    pub fn validate(&self) -> Result<(), String> {
        if self.mechanism != FLASH_MECHANISM {
            return Err(format!("flash reader mechanism must be {FLASH_MECHANISM}"));
        }
        if !plain_file(&self.artifact) || !plain_file(&self.image) {
            return Err("flash reader files must be plain names beside the facts".into());
        }
        if !sha256_text(&self.artifact_sha256) || !sha256_text(&self.image_sha256) {
            return Err("flash reader hashes must be lowercase sha256".into());
        }
        if self.image_bytes as usize <= crate::reader_scan::BOOTSTRAP_SLOT
            || self.image_bytes as usize > crate::reader_scan::MAX_IMAGE
        {
            return Err("flash reader image size is outside the EXR II ABI".into());
        }
        if self.review.trim().is_empty() {
            return Err("flash reader must name its review".into());
        }
        Ok(())
    }
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
        if let Some(reader) = &facts.flash_reader.value {
            reader.validate()?;
        }
        Ok(facts)
    }
    pub fn bundled() -> Self {
        Self::from_json(XE2_411).expect("checked-in facts parse")
    }
}
