use std::env;
use std::thread;
use std::time::Duration;

use audio_core::{
    AppPolicyClient, AudioEnumerator, AudioSession, ComGuard, DataFlow, DeviceRole, PolicyClient,
};

struct CliConfig {
    step: u8,
    device_match: String,
    app_match: String,
    target_vol: f32,
    hold_secs: u64,
}

fn parse_args() -> CliConfig {
    let mut cfg = CliConfig {
        step: 4,
        device_match: "HyperX".to_string(),
        app_match: "vivaldi.exe".to_string(),
        target_vol: 0.30,
        hold_secs: 5,
    };

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--step" => {
                if let Some(v) = args.next() {
                    if let Ok(parsed) = v.parse::<u8>() {
                        cfg.step = parsed;
                    }
                }
            }
            "--device" => {
                if let Some(v) = args.next() {
                    cfg.device_match = v;
                }
            }
            "--app" => {
                if let Some(v) = args.next() {
                    cfg.app_match = v;
                }
            }
            "--volume" => {
                if let Some(v) = args.next() {
                    if let Ok(parsed) = v.parse::<f32>() {
                        cfg.target_vol = parsed.clamp(0.0, 1.0);
                    }
                }
            }
            "--hold" => {
                if let Some(v) = args.next() {
                    if let Ok(parsed) = v.parse::<u64>() {
                        cfg.hold_secs = parsed;
                    }
                }
            }
            _ => {}
        }
    }

    cfg
}

fn run_step1(enumerator: &AudioEnumerator, cfg: &CliConfig) -> audio_core::Result<()> {
    println!("=== Step 1: WASAPI Session Volume Apply & Undo ===");
    println!(
        "Target device pattern: \"{}\" | Target app: \"{}\" | Target volume: {:.2} | Hold: {}s\n",
        cfg.device_match, cfg.app_match, cfg.target_vol, cfg.hold_secs
    );

    let endpoints = enumerator.list_active_endpoints(DataFlow::Render)?;
    println!("Active Playback Endpoints ({}):", endpoints.len());
    for (idx, ep) in endpoints.iter().enumerate() {
        println!("  [{idx}] {}  (id: {})", ep.friendly_name, ep.id);
    }

    let needle = cfg.device_match.to_lowercase();
    let Some(ep) = endpoints
        .iter()
        .find(|ep| ep.friendly_name.to_lowercase().contains(&needle))
    else {
        eprintln!(
            "\nNo active playback endpoint matched \"{}\".",
            cfg.device_match
        );
        return Ok(());
    };
    println!("\nMatched Endpoint: {}", ep.friendly_name);

    let sessions = ep.sessions()?;
    println!(
        "\nActive Audio Sessions on \"{}\" ({}):",
        ep.friendly_name,
        sessions.len()
    );

    let app_needle = cfg.app_match.to_lowercase();
    let mut matched: Vec<(AudioSession, f32)> = Vec::new();
    for (idx, session) in sessions.into_iter().enumerate() {
        let vol = session.volume()?;
        let muted = session.is_muted()?;
        println!(
            "  [{idx}] pid={:<6} exe={:<24} vol={vol:.2} muted={muted}",
            session.pid, session.exe_name
        );
        if session.exe_name.to_lowercase() == app_needle {
            matched.push((session, vol));
        }
    }

    if matched.is_empty() {
        eprintln!("\nNo active audio session matched \"{}\".", cfg.app_match);
        return Ok(());
    }

    println!(
        "\nApplying volume {:.2} to {} matched session(s)...",
        cfg.target_vol,
        matched.len()
    );
    for (session, orig_vol) in &matched {
        session.set_volume(cfg.target_vol)?;
        let readback = session.volume()?;
        println!(
            "  [APPLY] pid={} ({}): {orig_vol:.2} -> {readback:.2}",
            session.pid, session.exe_name
        );
    }

    println!("\nHolding for {} seconds before restoring...", cfg.hold_secs);
    thread::sleep(Duration::from_secs(cfg.hold_secs));

    println!("Restoring original volume(s)...");
    for (session, orig_vol) in &matched {
        session.set_volume(*orig_vol)?;
        let readback = session.volume()?;
        println!(
            "  [UNDO]  pid={} ({}): -> {readback:.2}",
            session.pid, session.exe_name
        );
    }

    Ok(())
}

fn run_step2(enumerator: &AudioEnumerator, cfg: &CliConfig) -> audio_core::Result<()> {
    println!("=== Step 2: Master Endpoint Volume/Mute & Capture (Mic) Enumeration ===");
    println!(
        "Target render pattern: \"{}\" | Test master volume: {:.2} | Hold: {}s\n",
        cfg.device_match, cfg.target_vol, cfg.hold_secs
    );

    let render_eps = enumerator.list_active_endpoints(DataFlow::Render)?;
    println!("Active Playback (eRender) Endpoints ({}):", render_eps.len());
    for (idx, ep) in render_eps.iter().enumerate() {
        let state = ep.master_volume()?;
        println!(
            "  [{idx}] {:<48} vol={:.2} muted={}",
            ep.friendly_name, state.volume, state.muted
        );
    }

    let capture_eps = enumerator.list_active_endpoints(DataFlow::Capture)?;
    println!(
        "\nActive Capture / Mic (eCapture) Endpoints ({}):",
        capture_eps.len()
    );
    for (idx, ep) in capture_eps.iter().enumerate() {
        let state = ep.master_volume()?;
        println!(
            "  [{idx}] {:<48} vol={:.2} muted={}  (id: {})",
            ep.friendly_name, state.volume, state.muted, ep.id
        );
    }

    let needle = cfg.device_match.to_lowercase();
    let Some(ep) = render_eps
        .iter()
        .find(|ep| ep.friendly_name.to_lowercase().contains(&needle))
    else {
        eprintln!(
            "\nNo active playback endpoint matched \"{}\".",
            cfg.device_match
        );
        return Ok(());
    };

    let initial = ep.master_volume()?;
    println!(
        "\nTesting master volume & mute apply/undo on \"{}\":",
        ep.friendly_name
    );
    println!(
        "  Initial state: vol={:.2}, muted={}",
        initial.volume, initial.muted
    );

    ep.set_master_volume(cfg.target_vol)?;
    ep.set_master_mute(true)?;
    let applied = ep.master_volume()?;
    println!("  [APPLY] vol={:.2}, muted={}", applied.volume, applied.muted);

    println!("  Holding for {} seconds before restoring...", cfg.hold_secs);
    thread::sleep(Duration::from_secs(cfg.hold_secs));

    ep.set_master_volume(initial.volume)?;
    ep.set_master_mute(initial.muted)?;
    let restored = ep.master_volume()?;
    println!(
        "  [UNDO]  vol={:.2}, muted={}",
        restored.volume, restored.muted
    );

    Ok(())
}

fn run_step3(enumerator: &AudioEnumerator, cfg: &CliConfig) -> audio_core::Result<()> {
    let target_pattern = if cfg.device_match == "PC Primary" {
        "HyperX"
    } else {
        cfg.device_match.as_str()
    };

    println!("=== Step 3: OS Default Device Role Switching (IPolicyConfig) ===");
    println!(
        "Target switch pattern: \"{target_pattern}\" | Hold: {}s\n",
        cfg.hold_secs
    );

    let render_eps = enumerator.list_active_endpoints(DataFlow::Render)?;
    let name_for_id = |id: &str| -> String {
        render_eps
            .iter()
            .find(|ep| ep.id == id)
            .map(|ep| ep.friendly_name.clone())
            .unwrap_or_else(|| id.to_string())
    };

    let policy = PolicyClient::new()?;
    let mut orig_ids = Vec::new();

    println!("Initial Default Playback Endpoints:");
    for role in DeviceRole::ALL {
        let id = enumerator.default_endpoint_id(DataFlow::Render, role)?;
        println!("  {role:<15?}: {}  ({id})", name_for_id(&id));
        orig_ids.push((role, id));
    }

    let needle = target_pattern.to_lowercase();
    let Some(target_ep) = render_eps
        .iter()
        .find(|ep| ep.friendly_name.to_lowercase().contains(&needle))
    else {
        eprintln!("\nNo active playback endpoint matched \"{target_pattern}\".");
        return Ok(());
    };

    println!(
        "\nSwitching all roles to: {} ({})",
        target_ep.friendly_name, target_ep.id
    );
    for role in DeviceRole::ALL {
        policy.set_default_endpoint(&target_ep.id, role)?;
        let readback = enumerator.default_endpoint_id(DataFlow::Render, role)?;
        println!("  [APPLY] {role:<15?}: {}", name_for_id(&readback));
    }

    println!("\nHolding for {} seconds before restoring...", cfg.hold_secs);
    thread::sleep(Duration::from_secs(cfg.hold_secs));

    println!("Restoring original default endpoints...");
    for (role, orig_id) in &orig_ids {
        policy.set_default_endpoint(orig_id, *role)?;
        let readback = enumerator.default_endpoint_id(DataFlow::Render, *role)?;
        println!("  [UNDO]  {role:<15?}: {}", name_for_id(&readback));
    }

    Ok(())
}

fn run_step4(enumerator: &AudioEnumerator, cfg: &CliConfig) -> audio_core::Result<()> {
    println!("=== Step 4: Per-App Output Device Routing (IAudioPolicyConfigFactory) ===");
    println!(
        "Target app: \"{}\" | Target output pattern: \"{}\" | Hold: {}s\n",
        cfg.app_match, cfg.device_match, cfg.hold_secs
    );

    let app_policy = AppPolicyClient::new()?;
    let render_eps = enumerator.list_active_endpoints(DataFlow::Render)?;
    let needle = cfg.device_match.to_lowercase();
    let Some(target_ep) = render_eps
        .iter()
        .find(|ep| ep.friendly_name.to_lowercase().contains(&needle))
    else {
        eprintln!(
            "No active playback endpoint matched \"{}\".",
            cfg.device_match
        );
        return Ok(());
    };

    let app_needle = cfg.app_match.to_lowercase();
    let mut found_pids: Vec<(u32, String)> = Vec::new();
    for ep in &render_eps {
        for s in ep.sessions()? {
            if s.exe_name.to_lowercase() == app_needle
                && !found_pids.iter().any(|(p, _)| *p == s.pid)
            {
                found_pids.push((s.pid, ep.friendly_name.clone()));
            }
        }
    }

    if found_pids.is_empty() {
        eprintln!(
            "No active audio session found for \"{}\" on any playback endpoint.",
            cfg.app_match
        );
        return Ok(());
    }

    let (pid, ep_name) = &found_pids[0];
    let pid = *pid;
    let orig_mm = app_policy.get_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia)?;
    println!("Found pid={pid} ({}) on \"{ep_name}\" (persisted=\"{orig_mm}\")", cfg.app_match);

    println!(
        "\nRouting pid={pid} ({}) to \"{}\"...",
        cfg.app_match, target_ep.friendly_name
    );
    app_policy.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Console, &target_ep.id)?;
    app_policy.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia, &target_ep.id)?;

    thread::sleep(Duration::from_millis(500));
    let readback_mm =
        app_policy.get_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia)?;
    println!("  [APPLY] persisted Multimedia endpoint = \"{readback_mm}\"");

    println!(
        "\nHolding for {} seconds (listen for audio on {})...",
        cfg.hold_secs, target_ep.friendly_name
    );
    thread::sleep(Duration::from_secs(cfg.hold_secs));

    println!("Restoring original persisted routing for pid={pid}...");
    app_policy.clear_all_app_endpoints()?;
    let restored_mm =
        app_policy.get_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia)?;
    println!("  [UNDO]  persisted Multimedia endpoint = \"{restored_mm}\"");

    Ok(())
}

fn run() -> audio_core::Result<()> {
    let cfg = parse_args();
    let _com = ComGuard::new()?;
    let enumerator = AudioEnumerator::new()?;

    match cfg.step {
        1 => run_step1(&enumerator, &cfg)?,
        2 => run_step2(&enumerator, &cfg)?,
        3 => run_step3(&enumerator, &cfg)?,
        4 => run_step4(&enumerator, &cfg)?,
        other => {
            eprintln!("Unknown --step {other}. Supported: 1, 2, 3, 4");
            std::process::exit(2);
        }
    }

    println!("\nSteel thread complete.");
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
