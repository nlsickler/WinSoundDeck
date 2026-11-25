use std::fs;
use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::daemon_ipc::CREATE_NO_WINDOW;

const TROUBLESHOOT_SCRIPT: &str = include_str!("../../../troubleshoot.ps1");
const GITHUB_ISSUES_NEW_URL: &str = "https://github.com/nlsickler/WinSoundDeck/issues/new";

pub fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn build_github_issue_url(version: &str) -> String {
    let title = format!("[Bug v{version}]: ");
    let body = "### Describe the issue\n<!-- What happened, and what did you expect? -->\n\n### Steps to reproduce\n1. \n\n### Diagnostic Report\nPlease attach the generated diagnostic `.md` file (its path has been copied to your clipboard — click **\"Paste, drop, or click to add files\"** below and paste it into the file picker).\n";
    format!(
        "{GITHUB_ISSUES_NEW_URL}?title={}&body={}",
        percent_encode_query(&title),
        percent_encode_query(body)
    )
}

#[tauri::command]
pub async fn run_troubleshoot_to_clipboard(open_github_issue: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let temp_dir = std::env::temp_dir();
        let pid = std::process::id();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let script_path = temp_dir.join(format!("winsounddeck_troubleshoot_{pid}.ps1"));
        let report_path = temp_dir.join(format!("winsounddeck-diagnostics-{stamp}.md"));

        fs::write(&script_path, format!("\u{FEFF}{TROUBLESHOOT_SCRIPT}"))
            .map_err(|e| format!("Failed to write temporary troubleshoot script: {e}"))?;

        let run_res = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script_path.to_string_lossy(),
                "-OutFile",
                &report_path.to_string_lossy(),
            ])
            .current_dir(&temp_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run troubleshoot script: {e}"));

        let _ = fs::remove_file(&script_path);
        let output = run_res?;
        if !output.status.success() {
            let _ = fs::remove_file(&report_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Troubleshoot script failed: {stderr}"));
        }

        let report_str = report_path.to_string_lossy().to_string();
        let escaped_path = report_str.replace('\'', "''");
        let clip_status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!("Set-Clipboard -Value '{escaped_path}'"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to copy report path to clipboard: {e}"))?;

        if !clip_status.success() {
            return Err("Failed to copy diagnostic report path to clipboard".to_string());
        }

        if open_github_issue {
            let url = build_github_issue_url(env!("CARGO_PKG_VERSION"));
            let ps_url = url.replace('\'', "''");
            Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &format!("Start-Process '{ps_url}'")])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("Report saved, but failed to open GitHub: {e}"))?;
            Ok(format!(
                "Report saved & path copied to clipboard — attach it to the GitHub Issue:\n{report_str}"
            ))
        } else {
            Ok(format!(
                "Report saved & path copied to clipboard (attach to a bug report):\n{report_str}"
            ))
        }
    })
    .await
    .map_err(|e| format!("Diagnostics task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::{build_github_issue_url, percent_encode_query};

    #[test]
    fn percent_encode_query_escapes_reserved_and_whitespace() {
        assert_eq!(
            percent_encode_query("a b&c=1\n"),
            "a%20b%26c%3D1%0A"
        );
        assert_eq!(percent_encode_query("v0.9.0-rc_1~"), "v0.9.0-rc_1~");
    }

    #[test]
    fn github_issue_url_includes_version_without_local_paths() {
        let url = build_github_issue_url("0.9.0");
        assert!(url.starts_with("https://github.com/nlsickler/WinSoundDeck/issues/new?title="));
        assert!(url.contains("0.9.0"));
        assert!(url.contains("body="));
        assert!(!url.contains("AppData"));
    }
}
