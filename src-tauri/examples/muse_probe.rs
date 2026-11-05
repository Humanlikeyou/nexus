//! First-contact check for a real Muse, with no app and no network.
//!
//!   cd src-tauri && cargo run --example muse_probe
//!
//! Scans for up to 30 s, connects to the first headset named Muse, logs its GATT table, then
//! records raw notifications for 30 s under preset p50 (what the app sends) and 30 s under p20
//! (the published EEG + AUX preset), and disconnects. Files go to `<app data>/muse/`, private to
//! this user: `muse-gatt-<date>.log` and `muse-capture-<time>.jsonl`.
//! Writes to the headset only the app's own commands: a preset, `s`, `d` and `h`.

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use nexus_lib::test_support::muse::{
    append_gatt_log, characteristic_name, is_control_characteristic, is_data_characteristic,
    muse_cmd, muse_data_dir,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

const SCAN_LIMIT: Duration = Duration::from_secs(30);
const CONNECT_LIMIT: Duration = Duration::from_secs(20);
const PHASES: [(&str, Duration); 2] = [
    ("p50", Duration::from_secs(30)),
    ("p20", Duration::from_secs(30)),
];

async fn find_muse(adapter: &Adapter) -> Result<Option<Peripheral>, String> {
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| format!("scan failed: {e}"))?;
    let deadline = Instant::now() + SCAN_LIMIT;
    let found = 'scan: loop {
        for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
            let name = p
                .properties()
                .await
                .ok()
                .flatten()
                .and_then(|x| x.local_name);
            if name.is_some_and(|n| n.to_lowercase().contains("muse")) {
                break 'scan Some(p);
            }
        }
        if Instant::now() >= deadline {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let _ = adapter.stop_scan().await;
    Ok(found)
}

async fn send(peripheral: &Peripheral, cmd: &str) -> Result<(), String> {
    let chars = peripheral.characteristics();
    let control = chars
        .iter()
        .find(|c| is_control_characteristic(&c.uuid))
        .ok_or("no control characteristic")?;
    peripheral
        .write(control, &muse_cmd(cmd), WriteType::WithoutResponse)
        .await
        .map_err(|e| format!("write {cmd} failed: {e}"))?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    Ok(())
}

async fn capture(peripheral: &Peripheral) -> Result<(), String> {
    peripheral
        .discover_services()
        .await
        .map_err(|e| format!("discovery failed: {e}"))?;
    let chars = peripheral.characteristics();
    let dir = muse_data_dir();
    let listing: Vec<_> = chars.iter().map(|c| (c.service_uuid, c.uuid)).collect();
    let log = append_gatt_log(&dir, &listing).map_err(|e| e.to_string())?;
    println!(
        "{} characteristics, logged to {}",
        chars.len(),
        log.display()
    );

    let path = dir.join(format!(
        "muse-capture-{}.jsonl",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    ));
    let file = nexus_lib::test_support::private_open_options()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut out = BufWriter::new(file);

    let mut notifications = peripheral
        .notifications()
        .await
        .map_err(|e| format!("notifications failed: {e}"))?;
    for ch in &chars {
        let wanted = is_data_characteristic(&ch.uuid) || is_control_characteristic(&ch.uuid);
        if wanted {
            let name = characteristic_name(&ch.uuid);
            match peripheral.subscribe(ch).await {
                Ok(()) => println!("subscribed {name}"),
                Err(e) => println!("could not subscribe {name}: {e}"),
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(250)).await;

    let start = Instant::now();
    'phases: for (preset, length) in PHASES {
        println!("recording {}s with preset {preset}", length.as_secs());
        let marker =
            json!({"event": "phase", "preset": preset, "t_ms": start.elapsed().as_millis() as u64});
        writeln!(out, "{marker}").map_err(|e| e.to_string())?;
        for cmd in [preset, "s", "d"] {
            send(peripheral, cmd).await?;
        }
        let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
        let deadline = tokio::time::Instant::now() + length;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = tokio::signal::ctrl_c() => {
                    println!("stopping early");
                    print_counts(preset, &counts);
                    break 'phases;
                }
                n = notifications.next() => {
                    let Some(n) = n else {
                        println!("the headset stopped sending");
                        print_counts(preset, &counts);
                        break 'phases;
                    };
                    let stream = characteristic_name(&n.uuid);
                    *counts.entry(stream).or_default() += 1;
                    let line = json!({
                        "t_ms": start.elapsed().as_millis() as u64,
                        "stream": stream,
                        "uuid": n.uuid.to_string(),
                        "hex": hex::encode(&n.value),
                    });
                    writeln!(out, "{line}").map_err(|e| e.to_string())?;
                }
            }
        }
        print_counts(preset, &counts);
        send(peripheral, "h").await?;
    }
    out.flush().map_err(|e| e.to_string())?;
    println!("capture saved to {}", path.display());
    Ok(())
}

fn print_counts(preset: &str, counts: &BTreeMap<&str, u64>) {
    println!("packets with {preset}: {counts:?}");
}

async fn run() -> Result<bool, String> {
    let manager = Manager::new().await.map_err(|e| e.to_string())?;
    let adapter = manager
        .adapters()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or("no Bluetooth adapter")?;
    println!("scanning for a Muse, up to {} s", SCAN_LIMIT.as_secs());
    let Some(peripheral) = find_muse(&adapter).await? else {
        println!("no Muse found");
        return Ok(false);
    };
    println!("found a Muse, connecting");
    tokio::time::timeout(CONNECT_LIMIT, peripheral.connect())
        .await
        .map_err(|_| "connect timed out".to_string())?
        .map_err(|e| format!("connect failed: {e}"))?;
    let captured = capture(&peripheral).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), peripheral.disconnect()).await;
    println!("disconnected");
    captured.map(|()| true)
}

#[tokio::main]
async fn main() {
    let code = match run().await {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    std::process::exit(code);
}
