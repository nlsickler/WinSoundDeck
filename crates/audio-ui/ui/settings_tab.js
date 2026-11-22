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
