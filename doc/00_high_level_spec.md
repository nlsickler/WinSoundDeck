# **Audio Preset & Device Routing Manager**

# **Personal Hack Specification & Agent Execution Plan**

---

## **1\. Executive Summary & Problem Statement**

Modern Windows desktop environments lack a lightweight native solution for orchestrating dynamic audio routing, application-level volume scaling, and hot-swappable hardware presets. Existing solutions across the landscape fall short for personal power-user workflows due to key architectural friction:

* **Flaws of Socket-Based Virtual Cable Loopbacks:** Tools relying on virtual audio cables or local TCP/gRPC loopback sockets (e.g., MixLine architectures) introduce non-trivial buffer latency, audio artifacts, and driver instability upon device sleep state transitions.  
* **Limitations of Consumer UI Layer Tools:** Utilities like EarTrumpet offer convenient user interfaces for active session volumes but lack state persistence across hardware hot-plug events, contextual global profile switches (e.g., *Focus*, *Call*, *Media*), or automated rule engines for newly spawned background processes.  
* **Overhead of Commercial Mixers:** Software suites like Voicemeeter impose dynamic kernel-driver overhead, complex setup, and overly complex visual abstractions for users who simply need deterministic, policy-based audio routing and dynamic gain staging.

The **Audio Preset & Device Routing Manager** is a pragmatic personal hacking project where the codebase will be generated primarily by AI agents with minimal manual coding. Priority is placed on crisp scope definition, fast failure, and pragmatic architecture. By directly leveraging the Windows Core Audio COM API architecture without kernel-mode driver modifications or network loopbacks, the system guarantees zero-overhead execution and seamless hot-swap resilience.

---

## **2\. Architecture Overview**

### **Platform Boundary & API Strategy**

The application operates entirely within user mode, directly interfacing with the **Windows Core Audio APIs (WASAPI)** via Component Object Model (COM) interfaces.

```
┌─────────────────────────────────────────────────────────────────┐
│                       User Interface (UI)                       │
│           System Tray / Tauri / Lightweight Native GUI          │
└────────────────────────────────▲────────────────────────────────┘
                                 │ Inter-Process Communication
                                 │ (Named Pipes / Native IPC)
┌────────────────────────────────▼────────────────────────────────┐
│                   Core Daemon Engine (Service)                  │
│  ┌────────────────────────┐         ┌────────────────────────┐  │
│  │ IMMNotificationClient  │         │IAudioSessionNotification│ │
│  │ (Device Hot-Plug Engine)│         │(Session Spawn Intercept)│ │
│  └───────────┬────────────┘         └───────────┬────────────┘  │
│              │                                  │               │
│              ▼                                  ▼               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │           Core Audio API Engine (COM / WASAPI)            │  │
│  │   IMMDeviceEnumerator  │  IAudioSessionManager2          │  │
│  │   ISimpleAudioVolume   │  IAudioEndpointVolume           │  │
│  └───────────────────────────────────────────────────────────┘  │
└────────────────────────────────▲────────────────────────────────┘
                                 │ Native COM Wrappers
┌────────────────────────────────▼────────────────────────────────┐
│                     Windows OS Kernel Audio                     │
└─────────────────────────────────────────────────────────────────┘
```

The system targets the following primary WASAPI interfaces:

* `IMMDeviceEnumerator`: Enumerates active, disabled, and disconnected audio endpoints (render/capture) and tracks system default endpoint updates.  
* `IAudioSessionManager2`: Intercepts and enumerates audio control sessions across specific hardware endpoints.  
* `ISimpleAudioVolume`: Controls per-application volume levels and mute states within specific audio sessions.  
* `IAudioEndpointVolume`: Controls master gain, channel balance, and mute hardware flags on specific `IMMDevice` nodes.

### **Core Engine vs UI Decoupling**

To maintain strict resource isolation and keep agent implementation clean, the architecture decouples execution into two distinct components:

1. **Headless Background Daemon:** Runs continuously in the background using minimal memory (\<15 MB target). It registers event listeners (`IMMNotificationClient`, `IAudioSessionNotification`) to react instantly to hardware updates and process launches without relying on polling loops.  
2. **Lightweight Desktop / Tray Interface:** Serves strictly as a visual controller and configuration interface. If the UI process terminates, the background engine maintains all volume rules, device states, and active routing policies seamlessly.

### **Technology Stack Selection**

| Layer | Primary Recommendation (Rust Core) | Secondary Option (.NET Core) |
| :---- | :---- | :---- |
| **Core Engine** | Rust (`windows-rs` crate) | C\# (.NET 8+ with NAudio / P/Invoke) |
| **COM Interop** | Native COM bindings via `windows` crate | Managed COM Interop via `comtypes` / Native wrappers |
| **UI Presentation** | Tauri (HTML/CSS/JS with system tray integration) | Native WPF / WinUI 3 (Lightweight system tray app) |
| **Memory Footprint** | \~5 MB to 12 MB | \~30 MB to 60 MB |
| **Execution Latency** | Direct native COM execution (\<1 ms overhead) | Managed runtime execution (\<3 ms overhead) |

---

## **3\. System Components**

### **Device State & Notification Engine**

This module maintains the internal real-time state of all available audio capture and render hardware. It implements the `IMMNotificationClient` interface to handle hardware events reactively:

* `OnDeviceStateChanged`: Detects when hardware is connected, unplugged, enabled, or disabled.  
* `OnDefaultDeviceChanged`: Updates active routing maps when the OS default audio render/capture path shifts.  
* `OnPropertyValueChanged`: Captures external volume changes (e.g., physical volume knobs on headsets or DACs).

### **Session Manager & Application Interceptor**

The Session Manager monitors active audio streams created by applications. To prevent incorrect identification of complex multi-window applications, session matching is explicitly bound to the process executable path and binary image name (`GetProcessImageFileName`), completely ignoring variable window titles.

* Registers `IAudioSessionNotification` on all active endpoint session managers.  
* Triggers automatic gain scaling and application routing rules whenever a target binary (e.g., `spotify.exe`, `ms-teams.exe`) opens an audio stream.

### **Preset Storage & Serialization Engine**

Provides a rigorous, unambiguous definition of the Audio Preset data contract, enforcement state machine, and conflict resolution rules to ensure deterministic execution across dynamic audio device events.

```json
{
  "version": "1.0.0",
  "active_preset_id": "preset_work_01",
  "presets": [
    {
      "id": "preset_work_01",
      "name": "Focus & Calls",
      "target_device_match": {
        "endpoint_id": "{0.0.0.00000000}.{a1b2c3d4-e5f6-7890-abcd-ef1234567890}",
        "friendly_name_pattern": "Headphone (Realtek Audio)"
      },
      "device_role": "DefaultCommunication",
      "master_settings": {
        "master_volume": 0.85,
        "master_mute": false
      },
      "default_fallback_volume": 1.0,
      "app_rules": [
        {
          "executable_name": "ms-teams.exe",
          "target_volume_scalar": 1.0,
          "is_muted": false
        },
        {
          "executable_name": "spotify.exe",
          "target_volume_scalar": 0.30,
          "is_muted": false
        }
      ]
    }
  ]
}
```

#### **1\. Preset Data Contract & Scope**

* **Device Binding & Match Strategy:** Presets explicitly target dynamic hardware endpoints by matching stable Windows Core Audio `Endpoint ID` strings and partial friendly device name patterns.  
* **Master Endpoint Gain & Mute:** Defines normalized hardware gain (`0.0` to `1.0`) and master hardware mute flags applied directly on the target endpoint.  
* **Default Fallback Application Volume:** Establishes a deterministic default volume scalar (e.g., `1.0`) for unlisted processes initializing audio streams on the bound device.  
* **Target Process Volume Map:** Maps normalized binary executable image names (e.g., `ms-teams.exe -> 1.0`, `spotify.exe -> 0.30`) to target gain scalars and mute parameters.  
* **Device Role Assignment:** Optionally configures Windows OS role routing assignments (e.g., `Default Multimedia` vs. `Default Communication` device roles).

#### **2\. Lifecycle & Enforcement Logic (State Machine)**

3. **Trigger Event:** Fired when a user initiates a profile switch OR when a target audio hardware endpoint connects or transitions to an active state.  
4. **Step 1 (Master Gain Application):** Set master endpoint volume levels and mute status via `IAudioEndpointVolume`.  
5. **Step 2 (Active Session Enumeration):** Iterate existing sessions via `IAudioSessionManager2` on the target device, match by image name, and apply target volume scalars immediately using `ISimpleAudioVolume`.  
6. **Step 3 (Table Registration):** Register active preset rules into an in-memory *Active Profile Enforcement Table*.  
7. **Step 4 (Reactive Catch):** When `IAudioSessionNotification` fires upon a late process stream launch, perform a lookup in the active enforcement table and clamp session volume prior to the first playback buffer execution.

#### **3\. Conflict & State Boundary Rules**

* **Manual Volume Adjustments:** Real-time manual slider adjustments made via Windows Volume Mixer or inline application controls override active session levels immediately, but do NOT mutate the underlying stored preset JSON file unless explicitly saved via the UI.  
* **Multi-Device Independence:** Each connected physical device maintains its own isolated `Active Preset ID`. Switching or modifying presets on Device A leaves Device B settings completely un-touched unless a global multi-device profile switch is triggered.

### **UI & System Tray Client**

The frontend interface provides intuitive control over complex audio routing:

* **System Tray Integration:** Fast access to quick-switch preset profiles via context menu.  
* **Visual Status Indicators:** Badges indicating real-time connection status (Connected, Disconnected, Fallback Active).  
* **Active Mixer Panel:** Displays master volume sliders along with real-time per-app volume sliders mapped to active process executables.

---

## **4\. Edge Cases & Resilience Strategy**

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       Session Interception Flow                            │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                         [ New Audio Session Created ]
                                       │
                                       ▼
                     Is Session Parent Multi-Process/Electron?
                                      / \
                                     /   \
                                   YES    NO
                                   /       \
                                  ▼         ▼
    Resolve Root Process via Toolhelp32   Direct Executable Matching
    Inspect Image Name (e.g. chrome.exe)  (e.g. custom_app.exe)
                                  \         /
                                   \       /
                                    ▼     ▼
                        ┌─────────────────────────┐
                        │   Match Active Preset   │
                        │    & Evaluate Rules     │
                        └────────────┬────────────┘
                                     │
                                     ▼
                    Application Currently Producing Audio?
                                    / \
                                   /   \
                                 YES    NO
                                 /       \
                                ▼         ▼
                  Apply Volume    Register Ephemeral Session Listener
                  Immediately     (Apply Rules upon Audio Stream Start)
```

### **Multi-Process & Electron Application Handling**

Modern frameworks (Chromium, Electron, Teams, Spotify) spawn multiple worker processes under different Process IDs (PIDs), while audio is often initialized by a secondary audio-renderer sub-process.

* **Resolution:** When an audio session initializes, the engine queries the process tree using `CreateToolhelp32Snapshot` and `OpenProcessToken` to resolve the root parent executable name. All sub-process sessions are aggregated and managed under a single executable domain rule (e.g., grouping all `chrome.exe` audio instances into one logical slider).

### **Ephemeral & Delayed Session Handling**

Applications like web browsers or communication platforms often initialize audio devices late or destroy sessions when idle, causing lost volume settings when audio resumes later.

* **Resolution:** The background daemon maintains an internal registry of *Pending Preset Policies*. When a process initializes an audio session 10 minutes after a preset is activated, the engine catches the `OnSessionCreated` event, checks the pending policy map, and applies the volume rules before the first audio frame reaches the output buffer.

### **Clean IPC & Local Execution**

* **Zero Socket IPC:** Avoids using local TCP, UDP, or gRPC loopback interfaces, eliminating overhead, port conflicts, and firewall complications.  
* **Native Windows IPC:** Inter-process communication between the background engine and the tray UI relies entirely on standard Windows Named Pipes or native COM RPC mechanisms for low overhead.

### **Audio Input Device (Microphone) Tracking**

* Provides full support for capture devices via `IAudioEndpointVolume`.  
* Tracks hardware gain levels, peak meters, and global mute flags independently of output paths.  
* Supports dedicated hardware mute notification hooks, maintaining sync between physical headset mute buttons and software status indicators.

---

## **5\. Steel Thread: High-Risk End-to-End Validation**

### **Why This Steel Thread First**

Validates the nastiest technical risk upfront: verifying whether agent-generated code can actually bind to Windows WASAPI COM interfaces in user mode, discover an active endpoint (e.g., Open Ear or Closed Ear headset), intercept an active audio session (e.g., Spotify or Teams), and force a volume scalar change without crashing, deadlocking, or locking audio handles.

### **Minimal Functional Slice**

* **Hardcoded Targets:** Hardcode target process (e.g., `spotify.exe` or `ms-teams.exe`) and target volume scalar (e.g., `0.30`) directly in code—no UI, no dynamic presets, no database persistence.  
* **Execution Environment:** Pure console/CLI application or barebones script/binary target.  
* **Endpoint Enumeration:** Enumerate the active playback endpoint using partial friendly name matching.  
* **Session Control Sequence:** Bind to `IAudioSessionManager2`, iterate active sessions, match target process image name, and invoke `ISimpleAudioVolume::SetMasterVolume`.  
* **Optional Quick-Probe:** Register `IAudioSessionNotification` to verify async callback firing when the target app begins streaming audio.

### **Corners Cut (Fail Fast Strategy)**

* No complex error recovery or retry loops.  
* Hardcoded device match strings.  
* Zero IPC implementation, zero UI elements.  
* Unwrapped raw COM calls / unsafe blocks allowed for immediate verification.  
* Immediate process exit on completion.

### **Risk Retirement Criteria**

The core technical risk is retired once the minimal binary executes and successfully forces a volume level change on an active session via WASAPI COM in user mode without requiring driver injection, loopback socket delays, or hitting COM threading deadlocks.