const { invoke } = window.__TAURI__.core;

export async function setupAppSettingsTab() {
  const inspectBtn = document.getElementById("inspect-btn");
  if (inspectBtn) {
    inspectBtn.onclick = async () => {
      try {
        await invoke("open_devtools");
      } catch (err) {
        console.error("open_devtools error:", err);
      }
    };
  }

  const copyBtn = document.getElementById("troubleshoot-copy-btn");
  const issueBtn = document.getElementById("troubleshoot-issue-btn");
  const statusEl = document.getElementById("troubleshoot-status");
  const runDiagnostics = async (openGithubIssue) => {
    if (copyBtn) copyBtn.disabled = true;
    if (issueBtn) issueBtn.disabled = true;
    if (statusEl) {
      statusEl.textContent = "Running diagnostics and copying report to clipboard...";
      statusEl.className = "settings-status";
    }
    try {
      const msg = await invoke("run_troubleshoot_to_clipboard", { openGithubIssue });
      if (statusEl) {
        statusEl.textContent = msg;
        statusEl.className = "settings-status settings-status-ok";
      }
    } catch (err) {
      if (statusEl) {
        statusEl.textContent = `Error: ${err}`;
        statusEl.className = "settings-status settings-status-err";
      }
    } finally {
      if (copyBtn) copyBtn.disabled = false;
      if (issueBtn) issueBtn.disabled = false;
    }
  };
  if (copyBtn) copyBtn.onclick = () => runDiagnostics(false);
  if (issueBtn) issueBtn.onclick = () => runDiagnostics(true);

  const cb = document.getElementById("launch-at-login-cb");
  if (!cb) return;

  try {
    cb.checked = Boolean(await invoke("get_launch_at_login"));
  } catch (err) {
    console.error("get_launch_at_login error:", err);
  }

  cb.onchange = async () => {
    const desired = cb.checked;
    try {
      cb.checked = Boolean(await invoke("set_launch_at_login", { enabled: desired }));
    } catch (err) {
      console.error("set_launch_at_login error:", err);
      cb.checked = !desired;
    }
  };
}
