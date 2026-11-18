//! Build-time check of the pinned EXR II flash reader.
//!
//! When `facts/xe2-411-service-facts.json` pins a reader (`flash_reader` with a
//! value and `confidence: high`), its artifact and image must sit beside the
//! facts, match the pinned sizes and SHA-256 hashes, and pass the static
//! screen in `src/reader_scan.rs` (no direct firmware calls, system calls,
//! coprocessor ops or NAND/DMA controller addresses). Otherwise the build
//! fails. Only a verified pair is embedded; with no pin, nothing is embedded
//! and the flash step refuses at run time.
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

#[path = "src/reader_scan.rs"]
#[allow(dead_code)]
mod reader_scan;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let facts_path = root.join("facts/xe2-411-service-facts.json");
    println!("cargo:rerun-if-changed={}", facts_path.display());
    println!("cargo:rerun-if-changed=src/reader_scan.rs");
    let facts: serde_json::Value =
        serde_json::from_slice(&fs::read(&facts_path).expect("read facts")).expect("parse facts");
    let fact = &facts["flash_reader"];
    let mut embedded = "pub const BUNDLED_READER: Option<(&[u8], &[u8])> = None;\n".to_string();
    if !fact["value"].is_null() && fact["confidence"] == "high" {
        let pin = &fact["value"];
        let text = |key: &str| {
            pin[key]
                .as_str()
                .unwrap_or_else(|| panic!("flash_reader.{key} missing"))
                .to_string()
        };
        let (artifact, image) = (text("artifact"), text("image"));
        for name in [&artifact, &image] {
            assert!(
                !name.contains(['/', '\\']) && !name.starts_with('.'),
                "flash reader file {name:?} must be a plain name beside the facts"
            );
        }
        let artifact_path = root.join("facts").join(&artifact);
        let image_path = root.join("facts").join(&image);
        println!("cargo:rerun-if-changed={}", artifact_path.display());
        println!("cargo:rerun-if-changed={}", image_path.display());
        let artifact_bytes = fs::read(&artifact_path).expect("pinned flash reader artifact");
        let image_bytes = fs::read(&image_path).expect("pinned flash reader image");
        assert_eq!(
            hex(&Sha256::digest(&artifact_bytes)),
            text("artifact_sha256"),
            "flash reader artifact differs from its pinned sha256"
        );
        assert_eq!(
            hex(&Sha256::digest(&image_bytes)),
            text("image_sha256"),
            "flash reader image differs from its pinned sha256"
        );
        assert_eq!(
            Some(image_bytes.len() as u64),
            pin["image_bytes"].as_u64(),
            "flash reader image size differs from its pin"
        );
        if let Err(findings) = reader_scan::scan(&image_bytes) {
            panic!(
                "flash reader image fails the static screen: {}",
                reader_scan::describe(&findings)
            );
        }
        embedded = format!(
            "pub const BUNDLED_READER: Option<(&[u8], &[u8])> = Some((include_bytes!({:?}), include_bytes!({:?})));\n",
            artifact_path.display().to_string(),
            image_path.display().to_string()
        );
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("bundled_reader.rs");
    fs::write(out, embedded).expect("write bundled reader");
}
