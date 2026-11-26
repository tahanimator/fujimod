//! Hand-authored service-capture facts with per-fact evidence.
//!
//! Rationale: see decision 0003 and decision 0004 in `docs/decisions`.
use serde::{Deserialize, Serialize};

/// The checked-in X-E2 4.11 facts. Hand-authored, never generated.
pub const XE2_411: &str = include_str!("../facts/xe2-411-service-facts.json");
/// The opt-in override inputs: the X70 1.10 values for the three unlocated
/// facts, injected only behind the `--assume-x70-values` flag. Not facts.
pub const XE2_411_X70_ASSUMED: &str =
    include_str!("../facts/xe2-411-service-facts.x70-assumed.json");
pub const FIRMWARE_ID: &str = "xe2-411";
/// The firmware the assumed values are borrowed from.
pub const X70_ASSUMED_SOURCE: &str = "x70-110";
/// The evidence prefix every assumed fact carries, so a run that used the
/// override is recognisable in the manifest and never mistaken for a located
/// fact. The override must never be written into the honest facts file.
pub const X70_ASSUMED_MARK: &str = "UNVERIFIED: borrowed from x70-110";

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
        facts.check_invariants()?;
        Ok(facts)
    }

    /// Shape checks that must hold for any value of the located facts, however
    /// they were set. Run at parse time and again after the opt-in override
    /// injects the three assumed X70 values, so an unsafe shape is refused
    /// whether it comes from the facts file or from the override inputs.
    pub fn check_invariants(&self) -> Result<(), String> {
        if let (Some(gate), Some(guard)) = (&self.read_gate.value, &self.reset_guard.value)
            && gate.offset == guard.offset
        {
            return Err("read gate and reset guard must be different bytes".into());
        }
        for gate in [&self.read_gate.value, &self.reset_guard.value]
            .into_iter()
            .flatten()
        {
            if gate.enabled_value == 0 {
                return Err("an enabled RAM gate value must be nonzero".into());
            }
        }
        if self.slow_read_bytes.value.is_some_and(|n| n == 0 || n > 4096) {
            return Err("slow_read_bytes must be 1..=4096".into());
        }
        if let Some(reader) = &self.flash_reader.value {
            reader.validate()?;
        }
        Ok(())
    }
    pub fn bundled() -> Self {
        Self::from_json(XE2_411).expect("checked-in facts parse")
    }

    /// The located facts that were supplied by the X70 override, not located
    /// on the X-E2: high-confidence facts whose evidence carries the
    /// [`X70_ASSUMED_MARK`]. Empty when the honest facts are in force.
    pub fn assumed_x70(&self) -> Vec<(&'static str, String)> {
        [
            ("read_gate", &self.read_gate.confidence, &self.read_gate.evidence),
            (
                "reset_guard",
                &self.reset_guard.confidence,
                &self.reset_guard.evidence,
            ),
            (
                "slow_read_bytes",
                &self.slow_read_bytes.confidence,
                &self.slow_read_bytes.evidence,
            ),
        ]
        .into_iter()
        .filter(|(_, conf, ev)| **conf == Confidence::High && ev.starts_with(X70_ASSUMED_MARK))
        .map(|(name, _, ev)| (name, ev.clone()))
        .collect()
    }
}

/// One borrowed value and where it came from. The provenance must begin with
/// [`X70_ASSUMED_MARK`]; the X70 source names the in-repo evidence.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssumedByte {
    pub value: RamByte,
    pub provenance: String,
    pub x70_source: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssumedSize {
    pub value: u32,
    pub provenance: String,
    pub x70_source: String,
}
fn assumed_evidence(provenance: &str, x70_source: &str) -> String {
    format!("{provenance} | X70 1.10 source: {x70_source}")
}

/// The opt-in override inputs: the X70 1.10 values for the three facts the
/// X-E2 analysis never located. These are not facts and are never written into
/// the honest facts file; they are injected into a live [`ServiceFacts`] only
/// when the user passes `--assume-x70-values`, and only for these three bytes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssumedX70Values {
    pub format: u32,
    pub about: String,
    pub firmware_id: String,
    pub borrowed_from: String,
    pub read_gate: AssumedByte,
    pub reset_guard: AssumedByte,
    pub slow_read_bytes: AssumedSize,
}

impl AssumedX70Values {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let v: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if v.format != 1 {
            return Err(format!("unsupported assumed-values format {}", v.format));
        }
        if v.firmware_id != FIRMWARE_ID {
            return Err(format!(
                "assumed values are for {}, this tool supports only {FIRMWARE_ID}",
                v.firmware_id
            ));
        }
        if v.borrowed_from != X70_ASSUMED_SOURCE {
            return Err(format!(
                "assumed values claim to be borrowed from {}, not {X70_ASSUMED_SOURCE}",
                v.borrowed_from
            ));
        }
        for (name, provenance) in [
            ("read_gate", &v.read_gate.provenance),
            ("reset_guard", &v.reset_guard.provenance),
            ("slow_read_bytes", &v.slow_read_bytes.provenance),
        ] {
            if !provenance.starts_with(X70_ASSUMED_MARK) {
                return Err(format!(
                    "assumed {name} provenance must begin with {X70_ASSUMED_MARK:?}"
                ));
            }
        }
        Ok(v)
    }

    pub fn bundled() -> Result<Self, String> {
        Self::from_json(XE2_411_X70_ASSUMED)
    }

    /// Inject the three borrowed values into a live `ServiceFacts`, as
    /// high-confidence facts whose evidence carries the assumed mark. Only
    /// these three bytes are touched; identity, code checks and the flash
    /// reader are left exactly as the honest facts file has them. The shared
    /// invariants are re-checked afterwards.
    pub fn apply_to(&self, facts: &mut ServiceFacts) -> Result<(), String> {
        facts.read_gate = Fact {
            value: Some(self.read_gate.value),
            confidence: Confidence::High,
            evidence: assumed_evidence(&self.read_gate.provenance, &self.read_gate.x70_source),
        };
        facts.reset_guard = Fact {
            value: Some(self.reset_guard.value),
            confidence: Confidence::High,
            evidence: assumed_evidence(&self.reset_guard.provenance, &self.reset_guard.x70_source),
        };
        facts.slow_read_bytes = Fact {
            value: Some(self.slow_read_bytes.value),
            confidence: Confidence::High,
            evidence: assumed_evidence(
                &self.slow_read_bytes.provenance,
                &self.slow_read_bytes.x70_source,
            ),
        };
        facts.check_invariants()
    }
}
