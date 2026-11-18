<#
.SYNOPSIS
    Headless diagnostic bundle generator for Audio Mixer WASAPI/UI hang investigation.
.DESCRIPTION
    Collects OS/hardware info, PnP AudioEndpoint/MEDIA/SoftwareComponent inventory,
    direct WASAPI IMMDeviceEnumerator probes (masks 0xF and 0xB in both STA and MTA),
    recent Application Hang / Audiosrv event logs, and Audio Mixer config/log files
    into a single timestamped Markdown report.
#>
[CmdletBinding()]
param(
    [string]$OutFile = ".\troubleshoot-report-$(Get-Date -Format 'yyyyMMdd-HHmmss').md"
)

$ErrorActionPreference = 'Continue'

function Invoke-WasapiProbe {
    param(
        [ValidateSet('STA', 'MTA')]
        [string]$Apartment,
        [int]$Mask
    )

    $probeScript = @'
param([int]$Mask)
$code = @"
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
class MMDeviceEnumeratorComObject {}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    int EnumAudioEndpoints(int dataFlow, int dwStateMask, out IMMDeviceCollection ppDevices);
}

[Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceCollection {
    int GetCount(out int pcDevices);
    int Item(int nDevice, out IMMDevice ppDevice);
}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    int Activate(ref Guid iid, int dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
    int OpenPropertyStore(int stgmAccess, out IPropertyStore ppProperties);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string ppstrId);
    int GetState(out int pdwState);
}

[Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IPropertyStore {
    int GetCount(out int cProps);
    int GetAt(int iProp, out PropertyKey pkey);
    int GetValue(ref PropertyKey key, out PropVariant pv);
}

[StructLayout(LayoutKind.Sequential)]
public struct PropertyKey { public Guid fmtid; public int pid; }

[StructLayout(LayoutKind.Sequential)]
public struct PropVariant {
    public short vt;
    short r1, r2, r3;
    public IntPtr p1;
    public IntPtr p2;
}

public class Probe {
    public static void Run(int mask) {
        var totalSw = Stopwatch.StartNew();
        var e = (IMMDeviceEnumerator)new MMDeviceEnumeratorComObject();
        IMMDeviceCollection c;
        int hrEnum = e.EnumAudioEndpoints(2, mask, out c);
        if (hrEnum != 0) {
            Console.WriteLine(string.Format("EnumAudioEndpoints(0x{0:X}) failed: 0x{1:X8}", mask, hrEnum));
            return;
        }
        int n;
        c.GetCount(out n);
        Console.WriteLine(string.Format("Mask=0x{0:X} Count={1}", mask, n));
        var k = new PropertyKey { fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"), pid = 14 };
        for (int i = 0; i < n; i++) {
            var sw = Stopwatch.StartNew();
            IMMDevice d;
            c.Item(i, out d);
            string id = "<unknown>";
            int st = -1;
            try { d.GetId(out id); } catch (COMException ex) { id = string.Format("ERR_ID(0x{0:X8})", ex.HResult); }
            try { d.GetState(out st); } catch {}
            Console.Write(string.Format("[{0}] State=0x{1:X} {2} -> ", i, st, id));
            try {
                IPropertyStore p;
                d.OpenPropertyStore(0, out p);
                PropVariant pv;
                p.GetValue(ref k, out pv);
                string name = pv.vt == 31 ? Marshal.PtrToStringUni(pv.p1) : "(none)";
                Console.WriteLine(string.Format("{0} ({1} ms)", name, sw.ElapsedMilliseconds));
            } catch (COMException ex) {
                Console.WriteLine(string.Format("ERROR 0x{0:X8} ({1} ms)", ex.HResult, sw.ElapsedMilliseconds));
            }
        }
        Console.WriteLine(string.Format("TotalElapsedMs={0}", totalSw.ElapsedMilliseconds));
    }
}
"@
Add-Type -TypeDefinition $code
[Probe]::Run($Mask)
'@

    $tmp = [System.IO.Path]::GetTempFileName() + ".ps1"
    try {
        Set-Content -Path $tmp -Value $probeScript -Encoding UTF8
        $flag = if ($Apartment -eq 'STA') { '-STA' } else { '-MTA' }
        $output = & powershell.exe -NoProfile $flag -ExecutionPolicy Bypass -File $tmp -Mask $Mask 2>&1
        return ($output | Out-String).TrimEnd()
    } finally {
        Remove-Item -Path $tmp -ErrorAction SilentlyContinue
    }
}

$sb = [System.Text.StringBuilder]::new()
$null = $sb.AppendLine("# Audio Mixer Diagnostic Bundle")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Generated: $(Get-Date -Format 'o')")
$null = $sb.AppendLine("")

# 1. System / OS Info
$null = $sb.AppendLine("## 1. System & OS Info")
$os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture
$cs = Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model
$sysInfo = [PSCustomObject]@{
    OS           = $os.Caption
    Version      = $os.Version
    Build        = $os.BuildNumber
    Architecture = $os.OSArchitecture
    Manufacturer = $cs.Manufacturer
    Model        = $cs.Model
    PSVersion    = $PSVersionTable.PSVersion.ToString()
}
$null = $sb.AppendLine('```json')
$null = $sb.AppendLine(($sysInfo | ConvertTo-Json))
$null = $sb.AppendLine('```')
$null = $sb.AppendLine("")

# 2. WASAPI Probes (0xF and 0xB in STA and MTA)
$null = $sb.AppendLine("## 2. WASAPI IMMDeviceEnumerator Probes")
foreach ($apt in @('STA', 'MTA')) {
    foreach ($mask in @(0xF, 0xB)) {
        $maskHex = ('0x{0:X}' -f $mask)
        $null = $sb.AppendLine("### $apt — Mask $maskHex")
        $null = $sb.AppendLine('```text')
        $null = $sb.AppendLine((Invoke-WasapiProbe -Apartment $apt -Mask $mask))
        $null = $sb.AppendLine('```')
        $null = $sb.AppendLine("")
    }
}

# 3. PnP Inventory
$null = $sb.AppendLine("## 3. PnP Audio & Media Inventory")
$null = $sb.AppendLine('```json')
$pnp = Get-PnpDevice -Class AudioEndpoint, MEDIA, SoftwareComponent -ErrorAction SilentlyContinue |
    Select-Object Class, Status, FriendlyName, Manufacturer, InstanceId |
    Sort-Object Class, Status, FriendlyName |
    ConvertTo-Json -Depth 3
$null = $sb.AppendLine($pnp)
$null = $sb.AppendLine('```')
$null = $sb.AppendLine("")

# 4. Event Logs (Application Hang & Audiosrv)
$null = $sb.AppendLine("## 4. Event Logs (Last 24 Hours)")
$since = (Get-Date).AddHours(-24)

$null = $sb.AppendLine("### Application Hang (Event ID 1002)")
$null = $sb.AppendLine('```text')
$hangs = Get-WinEvent -FilterHashtable @{
    LogName      = 'Application'
    ProviderName = 'Application Hang'
    StartTime    = $since
} -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'audio-ui|audio-daemon' -or $_.Id -eq 1002 } |
    Select-Object -First 10 TimeCreated, Id, Message |
    Format-List | Out-String
$null = $sb.AppendLine($(if ($hangs.Trim()) { $hangs.TrimEnd() } else { "(none)" }))
$null = $sb.AppendLine('```')
$null = $sb.AppendLine("")

$null = $sb.AppendLine("### Audio Service Errors / Warnings (System)")
$null = $sb.AppendLine('```text')
$audioEvents = Get-WinEvent -FilterHashtable @{
    LogName   = 'System'
    StartTime = $since
} -ErrorAction SilentlyContinue |
    Where-Object { $_.ProviderName -match 'Audio|Audiosrv|AudioEndpointBuilder' -and $_.Level -le 3 } |
    Select-Object -First 15 TimeCreated, ProviderName, Id, LevelDisplayName, Message |
    Format-List | Out-String
$null = $sb.AppendLine($(if ($audioEvents.Trim()) { $audioEvents.TrimEnd() } else { "(none)" }))
$null = $sb.AppendLine('```')
$null = $sb.AppendLine("")

# 5. Audio Mixer Config & Logs
$null = $sb.AppendLine("## 5. Audio Mixer Config & Logs")
$appDataDir = Join-Path $env:APPDATA "audio-mixer"
$localAppDataDir = Join-Path $env:LOCALAPPDATA "audio-mixer"
$null = $sb.AppendLine("- APPDATA path: ``$appDataDir``")
$null = $sb.AppendLine("- LOCALAPPDATA path: ``$localAppDataDir``")
$null = $sb.AppendLine("")

foreach ($file in @("presets.json", "app_settings.json")) {
    $p = Join-Path $appDataDir $file
    $null = $sb.AppendLine("### $file")
    if (Test-Path $p) {
        $null = $sb.AppendLine('```json')
        $null = $sb.AppendLine((Get-Content -Path $p -Raw))
        $null = $sb.AppendLine('```')
    } else {
        $null = $sb.AppendLine("*(not present)*")
    }
    $null = $sb.AppendLine("")
}

$logFiles = Get-ChildItem -Path $appDataDir, $localAppDataDir, "." -Filter "*.log" -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 5
foreach ($lf in $logFiles) {
    $null = $sb.AppendLine("### Log: $($lf.FullName) (last 100 lines)")
    $null = $sb.AppendLine('```text')
    $null = $sb.AppendLine(((Get-Content -Path $lf.FullName -Tail 100) -join "`n"))
    $null = $sb.AppendLine('```')
    $null = $sb.AppendLine("")
}

Set-Content -Path $OutFile -Value $sb.ToString() -Encoding UTF8
Write-Host "Diagnostic report written to: $(Resolve-Path $OutFile)"
