use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use audio_core::ipc::IpcRequest;

use crate::daemon_ipc::{ipc_call, CREATE_NO_WINDOW};

const RUN_REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_REG_VALUE: &str = "WinSoundDeck";

fn validate_endpoint_id(endpoint_id: &str) -> Result<(), String> {
    let valid = !endpoint_id.is_empty()
        && endpoint_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '{' | '}' | '.' | '-'));
    if !valid {
        return Err(format!("Invalid endpoint ID: '{endpoint_id}'"));
    }
    Ok(())
}

pub fn pnp_instance_id_for_endpoint(endpoint_id: &str) -> Result<String, String> {
    validate_endpoint_id(endpoint_id)?;
    Ok(format!("SWD\\MMDEVAPI\\{endpoint_id}"))
}

pub fn sound_properties_uri_for_endpoint(endpoint_id: &str) -> Result<String, String> {
    validate_endpoint_id(endpoint_id)?;
    Ok(format!("ms-settings:sound-properties?endpointId={endpoint_id}"))
}

pub fn format_startup_command(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.display())
}

#[tauri::command]
pub fn remove_windows_endpoint(endpoint_id: String) -> Result<(), String> {
    let instance_id = pnp_instance_id_for_endpoint(&endpoint_id)?;
    let script = format!(
        "$p = Start-Process -FilePath 'pnputil.exe' -ArgumentList @('/remove-device', '{instance_id}') -Verb RunAs -Wait -PassThru; exit $p.ExitCode"
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to launch elevated pnputil: {e}"))?;
    if !status.success() {
        return Err(format!(
            "pnputil /remove-device exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    ipc_call(IpcRequest::ForgetEndpoint { endpoint_id })?;
    Ok(())
}

#[tauri::command]
pub fn open_bluetooth_settings() -> Result<(), String> {
    Command::new("explorer.exe")
        .arg("ms-settings:bluetooth")
        .spawn()
        .map_err(|e| format!("Failed to open Bluetooth settings: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn open_sound_properties(endpoint_id: String) -> Result<(), String> {
    let uri = sound_properties_uri_for_endpoint(&endpoint_id)?;
    Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &format!("Start-Process '{uri}'")])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to open Windows Sound Properties: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_launch_at_login() -> Result<bool, String> {
    let output = Command::new("reg.exe")
        .args(["query", RUN_REG_KEY, "/v", RUN_REG_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to query Windows startup registry: {e}"))?;
    Ok(output.status.success())
}

#[tauri::command]
pub fn set_launch_at_login(enabled: bool) -> Result<bool, String> {
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve current executable: {e}"))?;
        let cmd = format_startup_command(&exe);
        let status = Command::new("reg.exe")
            .args([
                "add",
                RUN_REG_KEY,
                "/v",
                RUN_REG_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &cmd,
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to update Windows startup registry: {e}"))?;
        if !status.success() {
            return Err("Failed to enable Launch at Windows login".to_string());
        }
    } else {
        let status = Command::new("reg.exe")
            .args(["delete", RUN_REG_KEY, "/v", RUN_REG_VALUE, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to update Windows startup registry: {e}"))?;
        if !status.success() && get_launch_at_login().unwrap_or(false) {
            return Err("Failed to disable Launch at Windows login".to_string());
        }
    }
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::{
        format_startup_command, pnp_instance_id_for_endpoint, sound_properties_uri_for_endpoint,
    };
    use std::path::Path;

    #[test]
    fn pnp_instance_id_formats_valid_endpoint_and_rejects_injection() {
        let valid = "{0.0.0.00000000}.{2d70e42c-d838-48f4-966d-8623cd910f4f}";
        assert_eq!(
            pnp_instance_id_for_endpoint(valid).unwrap(),
            format!("SWD\\MMDEVAPI\\{valid}")
        );
        assert!(pnp_instance_id_for_endpoint("").is_err());
        assert!(pnp_instance_id_for_endpoint("foo'; calc.exe").is_err());
        assert_eq!(
            sound_properties_uri_for_endpoint(valid).unwrap(),
            format!("ms-settings:sound-properties?endpointId={valid}")
        );
        assert!(sound_properties_uri_for_endpoint("foo&bar").is_err());
    }

    #[test]
    fn format_startup_command_quotes_executable_path() {
        let exe = Path::new("C:\\Program Files\\WinSoundDeck\\WinSoundDeck.exe");
        assert_eq!(
            format_startup_command(exe),
            "\"C:\\Program Files\\WinSoundDeck\\WinSoundDeck.exe\""
        );
    }
}
