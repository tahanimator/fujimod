//! fujimod-xe2-capture: local read-only X-E2 4.11 service-mode capture.
//!
//!   fujimod-xe2-capture plan                 offline: print the plan and facts
//!   fujimod-xe2-capture dry-run              identify, check, print the plan; no writes
//!   fujimod-xe2-capture capture OUTPUT_DIR   dump camera state into a new folder
//!
//! Only a local USB device 04cb:ff80 is used. Remote USB shares are refused and
//! nothing is sent over the network.
use fujimod_catalog::ReferenceCatalog;
use fujimod_usb::{
    connection::{self, Connection, Device, DeviceFilter, TransferOptions},
    trace::Trace,
};
use fujimod_xe2_capture::{
    BATTERY_REMINDER, Inputs,
    facts::{FIRMWARE_ID, ServiceFacts, UsbIds},
    policy::{Auditor, Policy},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    num::NonZeroU32,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

type Failure = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = futures_lite::future::block_on(run(&args));
    println!("\nREMINDER: {BATTERY_REMINDER}");
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("\nSTOPPED: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &[String]) -> Result<(), Failure> {
    let usage = "usage: fujimod-xe2-capture plan | dry-run | capture OUTPUT_DIR";
    let facts = ServiceFacts::bundled();
    let selected = ReferenceCatalog::bundled()?.resolve_firmware(FIRMWARE_ID)?;
    let bulk = selected.bulk.ok_or("catalog has no xe2-411 bulk facts")?;
    let state = selected
        .state
        .ok_or("catalog has no xe2-411 state regions")?;
    let inputs = Inputs {
        facts: &facts,
        bulk: &bulk,
        state: &state,
        flash: selected.flash.as_ref(),
    };
    inputs.validate()?;
    match args.first().map(String::as_str) {
        Some("plan") if args.len() == 1 => {
            print_plan(&inputs);
            Ok(())
        }
        Some("dry-run") if args.len() == 1 => {
            let (mut session, usb, trace) = open().await?;
            let report = inputs.dry_run(&mut session, usb).await;
            let mut auditor = Auditor::new(Policy::read_only());
            auditor.absorb(trace.drain());
            let commands = auditor.finish()?;
            let report = report?;
            println!("audited {commands} FF80 commands: all read-only");
            println!("{}", serde_json::to_string_pretty(&report)?);
            print_plan(&inputs);
            Ok(())
        }
        Some("capture") if args.len() == 2 => capture(&inputs, Path::new(&args[1])).await,
        _ => Err(usage.into()),
    }
}

fn print_plan(inputs: &Inputs<'_>) {
    println!("\nPlanned command sequence for X-E2 4.11 (fujimod-xe2-capture):");
    for (n, step) in inputs.plan().iter().enumerate() {
        println!(
            "\n{}. {} [{}]",
            n + 1,
            step.name,
            if step.ready { "ready" } else { "REFUSED" }
        );
        for command in &step.commands {
            println!("     {command}");
        }
        for missing in &step.refused_because {
            println!("     missing fact `{}`: {}", missing.fact, missing.evidence);
        }
    }
}

/// Open the single local 04cb:ff80 device.
async fn open() -> Result<(fujimod_usb::ff80::Session, UsbIds, Trace), Failure> {
    if std::env::var_os("FUJIMOD_USB_PROXY_URL").is_some() {
        return Err("unset FUJIMOD_USB_PROXY_URL: this tool only uses a local USB camera".into());
    }
    let devices: Vec<Device> = connection::discover(DeviceFilter::Diagnostic)
        .await?
        .into_iter()
        .filter(|d| matches!(d, Device::Local(_)))
        .filter(|d| d.vendor_id() == 0x04cb && d.product_id() == 0xff80)
        .collect();
    if devices.len() != 1 {
        return Err(format!(
            "expected exactly one X-E2 in service mode (USB 04cb:ff80), found {}. Enter \
             service mode with USB connected, and bind WinUSB with Zadig on Windows.",
            devices.len()
        )
        .into());
    }
    let device = &devices[0];
    let usb = UsbIds {
        vendor_id: device.vendor_id(),
        product_id: device.product_id(),
    };
    let interfaces = connection::interfaces(device).await?;
    if interfaces.len() != 1 {
        return Err("expected one FF80 interface".into());
    }
    let connection = Connection::open(
        device,
        interfaces[0],
        TransferOptions {
            timeout: Duration::from_secs(10),
            max_reply_bytes: NonZeroU32::new(2 << 20).expect("nonzero"),
        },
    )
    .await?;
    let trace = Trace::new(
        NonZeroU32::new(1 << 20).expect("nonzero"),
        NonZeroU32::new(64 << 20).expect("nonzero"),
    );
    let session = connection.begin_session(Some(trace.clone())).await?;
    Ok((session, usb, trace))
}

async fn capture(inputs: &Inputs<'_>, output: &Path) -> Result<(), Failure> {
    // Refuse missing facts before touching USB.
    if let Some(step) = inputs
        .plan()
        .into_iter()
        .find(|s| s.name == "dump camera state" && !s.ready)
    {
        print_plan(inputs);
        return Err(fujimod_xe2_capture::Error::Missing(step.refused_because).into());
    }
    fs::create_dir(output).map_err(|e| format!("{}: {e} (use a new folder)", output.display()))?;
    let (mut session, usb, trace) = open().await?;
    let mut save = |name: &str, bytes: &[u8]| -> Result<String, String> {
        let file = format!("{name}.bin");
        if !file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(format!("unsafe region name {name:?}"));
        }
        let path: PathBuf = output.join(&file);
        let mut handle = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        handle.write_all(bytes).map_err(|e| e.to_string())?;
        handle.sync_all().map_err(|e| e.to_string())?;
        // Hash the file as written, independently of the capture digest.
        let written = fs::read(&path).map_err(|e| e.to_string())?;
        if Sha256::digest(&written)[..] != Sha256::digest(bytes)[..] {
            return Err(format!("{file} differs from the captured bytes"));
        }
        println!("saved {file} ({} bytes)", bytes.len());
        Ok(file)
    };
    let policy = Policy {
        read_gate: inputs.facts.read_gate.value,
        reset_guard: inputs.facts.reset_guard.value,
    };
    let auditor = std::cell::RefCell::new(Auditor::new(policy));
    let mut last = 0;
    let mut progress = |done: u64, total: u64| {
        auditor.borrow_mut().absorb(trace.drain());
        let percent = done * 100 / total.max(1);
        if percent >= last + 5 || done == total {
            last = percent;
            println!("  {percent}% ({done}/{total} bytes)");
        }
    };
    let manifest = inputs
        .capture(&mut session, usb, &mut save, &mut progress)
        .await;
    let mut auditor = auditor.into_inner();
    auditor.absorb(trace.drain());
    let commands = auditor.finish()?;
    let manifest = manifest?;
    let mut json = serde_json::to_value(&manifest)?;
    json["audited_ff80_commands"] = commands.into();
    fs::write(
        output.join("manifest.json"),
        serde_json::to_string_pretty(&json)?,
    )?;
    println!("wrote {}", output.join("manifest.json").display());
    Ok(())
}
