//! Conservative static screen of an EXR II flash-reader image (A32).
//!
//! Shared by `build.rs` (build-time check of a pinned image) and the runtime
//! check before any upload, so it uses only `core`. The image layout is the
//! one `fujimod_workflows::flash::exr2::Payload::validate` fixes: a bootstrap
//! stub padded to 32 bytes that runs in borrowed scratch, then the reader
//! body that runs from its heap allocation, ending in an A32 `bx lr`.
//!
//! The reviewed reader reaches stock code only through function pointers that
//! the host writes into its request block (the stock page reader, the driver
//! lock and unlock, and the correction-mode entry, all pinned by catalog code
//! checks). Programming or erasing NAND would need one of: a direct call into
//! other firmware, a supervisor/system call, coprocessor or mode changes, or
//! an absolute address in the controller's I/O space. Every 32-bit word is
//! screened for those, whether it is an instruction or literal data, so a
//! literal that merely looks like one of them is also refused. The screen can
//! refuse a safe image; it cannot make an unsafe one acceptable on its own.
//! The pinned SHA-256 of the reviewed image is the primary guarantee.

/// Bytes reserved for the bootstrap stub; the reader body starts here.
pub const BOOTSTRAP_SLOT: usize = 32;
/// Largest image `exr2::Payload::validate` accepts (its request offset).
pub const MAX_IMAGE: usize = 0x1000;
/// A32 `bx lr`, the observer entry that must end the image.
pub const BX_LR: u32 = 0xe12f_ff1e;

/// EXR II peripheral address space, covering every controller block in
/// `catalog/soc/exr2/soc.json`, including the NAND controller at 0xfff52000
/// and the DMA engines at 0xfff02000. A test keeps this in step with the SoC.
pub const PERIPHERAL_SPACE: [(u32, u32); 2] =
    [(0xfe00_0000, 0xff00_0000), (0xfff0_0000, 0xfffc_0000)];

fn peripheral(value: u32) -> bool {
    PERIPHERAL_SPACE
        .iter()
        .any(|&(start, end)| value >= start && value < end)
}

/// Why one word was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub offset: usize,
    pub word: u32,
    pub reason: &'static str,
}

/// Screen one word at image offset `at` inside the segment `[start, end)`.
fn screen(word: u32, at: usize, start: usize, end: usize) -> Option<&'static str> {
    let cond = word >> 28;
    let op = (word >> 25) & 0b111;
    let top = (word >> 24) & 0xf;
    if peripheral(word) {
        return Some("absolute address in peripheral (NAND/DMA controller) space");
    }
    // MOVT Rd, #imm16 building the upper half of a peripheral address.
    if cond != 0xf && word & 0x0ff0_0000 == 0x0340_0000 {
        let high = ((word >> 4) & 0xf000) | (word & 0x0fff);
        if peripheral(high << 16) {
            return Some("MOVT builds a peripheral (NAND/DMA controller) address");
        }
    }
    if cond == 0xf {
        if op == 0b101 {
            return Some("BLX immediate (direct call with a mode switch)");
        }
        if op == 0b110 || top == 0b1110 {
            return Some("unconditional coprocessor instruction");
        }
        // CPS / SETEND / SRS / RFE and other system forms.
        if word & 0x0ff1_0020 == 0x0100_0000
            || word & 0x0e50_0000 == 0x0840_0000
            || word & 0x0e50_0000 == 0x0810_0000
        {
            return Some("processor state or exception-return instruction");
        }
        return None;
    }
    if top == 0xf {
        return Some("supervisor call (SVC)");
    }
    if op == 0b110 || top == 0b1110 {
        return Some("coprocessor instruction (cache, MPU, system control or VFP)");
    }
    // SMC: cond 0001 0110 .... .... .... 0111 ....
    if word & 0x0ff0_00f0 == 0x0160_0070 {
        return Some("secure monitor call (SMC)");
    }
    // MSR (immediate): cond 0011 0x10 ...; MSR (register): cond 0001 0x10 ... 0000 ....
    if word & 0x0fb0_0000 == 0x0320_0000 && word & 0x000f_0000 != 0 {
        return Some("MSR writes processor status");
    }
    if word & 0x0fb0_00f0 == 0x0120_0000 {
        return Some("MSR writes processor status");
    }
    if op == 0b101 {
        let imm = ((word & 0x00ff_ffff) << 8) as i32 >> 6;
        let target = at as i64 + 8 + i64::from(imm);
        if target < start as i64 || target >= end as i64 {
            return Some("direct branch leaves the uploaded code (call into firmware)");
        }
    }
    None
}

/// Screen a whole image. Returns every finding, in order; empty means it passed.
pub fn scan(image: &[u8]) -> Result<(), Vec<Finding>> {
    let mut findings = Vec::new();
    let mut fail = |offset, word, reason| {
        findings.push(Finding {
            offset,
            word,
            reason,
        })
    };
    if image.len() <= BOOTSTRAP_SLOT || image.len() > MAX_IMAGE || !image.len().is_multiple_of(4) {
        fail(
            0,
            0,
            "image size is not a word-aligned bootstrap slot plus reader of at most 4 KiB",
        );
        return Err(findings);
    }
    let words: Vec<u32> = image
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if words.last() != Some(&BX_LR) {
        fail(
            image.len() - 4,
            *words.last().unwrap_or(&0),
            "image does not end with the bx lr observer",
        );
    }
    for (index, &word) in words.iter().enumerate() {
        let at = index * 4;
        let (start, end) = if at < BOOTSTRAP_SLOT {
            (0, BOOTSTRAP_SLOT)
        } else {
            (BOOTSTRAP_SLOT, image.len())
        };
        if let Some(reason) = screen(word, at, start, end) {
            fail(at, word, reason);
        }
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings)
    }
}

/// One line per finding, for errors and build failures.
pub fn describe(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("+{:#05x} {:#010x}: {}", f.offset, f.word, f.reason))
        .collect::<Vec<_>>()
        .join("; ")
}
