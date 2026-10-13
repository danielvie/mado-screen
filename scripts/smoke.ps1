param([Parameter(Mandatory = $true)][string]$Exe)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PocSmoke {
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr hwnd, int index);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint thread, uint message, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool RegisterHotKey(IntPtr hwnd, int id, uint mods, uint key);
    [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr hwnd, int id);
}
'@
$oldDpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$directory = Join-Path $env:TEMP ("mado-poc1-" + [Guid]::NewGuid())
New-Item -ItemType Directory $directory | Out-Null
$copy = Join-Path $directory 'overlay-smoke.exe'
Copy-Item (Resolve-Path $Exe).Path $copy
$checks = [Collections.Generic.List[string]]::new()
$process = $null
$conflict = $null
$reserved = $false
function Check([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw "FAILED: $Name. Logs: $directory" }
    $checks.Add($Name)
    Write-Host "PASS: $Name"
}
function Pixel([int]$X, [int]$Y) {
    $bitmap = [Drawing.Bitmap]::new(1, 1)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($X, $Y, 0, 0, [Drawing.Size]::new(1, 1))
        return $bitmap.GetPixel(0, 0).ToArgb()
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function StartOverlay([string]$Prefix) {
    $child = Start-Process $copy -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $directory "$Prefix-out.log") -RedirectStandardError (Join-Path $directory "$Prefix-error.log")
    # Windows PowerShell loses redirected child exit codes unless the handle is cached.
    $null = $child.Handle
    return $child
}
try {
    Check ([PocSmoke]::FindWindow('MadoScreenPoc1', 'Mado Screen PoC 1') -eq [IntPtr]::Zero) 'No pre-existing overlay'
    $width = [PocSmoke]::GetSystemMetrics(0)
    $height = [PocSmoke]::GetSystemMetrics(1)
    $process = StartOverlay 'run'
    # Wait for native creation/composition, not for an agent or background workflow.
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    do {
        Start-Sleep -Milliseconds 50
        $window = [PocSmoke]::FindWindow('MadoScreenPoc1', 'Mado Screen PoC 1')
        $startup = [string](Get-Content (Join-Path $directory 'run-out.log') -Raw -ErrorAction SilentlyContinue)
        $ready = [PocSmoke]::IsWindowVisible($window) -and $startup.Contains('Mode: click-through.')
    } while (-not $ready -and -not $process.HasExited -and $deadline.Elapsed.TotalSeconds -lt 5)
    [uint32]$owner = 0
    $thread = [PocSmoke]::GetWindowThreadProcessId($window, [ref]$owner)
    Check ([PocSmoke]::IsWindowVisible($window) -and $owner -eq $process.Id) 'This executable creates and shows the overlay'
    $style = [PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64()
    Check (($style -band 0x080800a8) -eq 0x080800a8) ('Topmost layered click-through nonactivating tool window styles: 0x{0:x}' -f $style)
    $lx = [int][Math]::Floor($width / 8)
    $ly = [int][Math]::Floor($height / 4)
    $rx = [int][Math]::Floor($width / 2)
    $ry = [int][Math]::Floor($height / 3)
    Start-Sleep -Milliseconds 300
    $ink = [Drawing.Color]::FromArgb(255, 249, 115, 22).ToArgb()
    Check ((Pixel $lx $ly) -eq $ink) 'Automatic line visible on actual desktop'
    Check ((Pixel $rx $ry) -eq $ink) 'Automatic rectangle visible on actual desktop'
    $process.Refresh()
    $cpuStart = $process.TotalProcessorTime.TotalSeconds
    $watch = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds 30
    $process.Refresh()
    $cpuSeconds = $process.TotalProcessorTime.TotalSeconds - $cpuStart
    $idlePercent = 100 * $cpuSeconds / $watch.Elapsed.TotalSeconds / [Environment]::ProcessorCount
    Check ($idlePercent -lt 1) 'Idle CPU below 1 percent over 30 seconds'
    # Queue clear/quit directly. This intentionally does NOT prove real keyboard routing.
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(2), [IntPtr]::Zero)) 'Clear action queued'
    Start-Sleep -Milliseconds 500
    Check ((Pixel $lx $ly) -ne $ink -and (Pixel $rx $ry) -ne $ink) 'Clear removes visible shape pixels'
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(3), [IntPtr]::Zero)) 'Quit action queued'
    Check ($process.WaitForExit(5000) -and $process.ExitCode -eq 0) 'Quit exits normally'
    Check ([PocSmoke]::FindWindow('MadoScreenPoc1', 'Mado Screen PoC 1') -eq [IntPtr]::Zero) 'Quit removes overlay window'
    $reserved = [PocSmoke]::RegisterHotKey([IntPtr]::Zero, 200, 0x4003, 0x51)
    Check $reserved 'Quit shortcut released after normal exit'
    $conflict = StartOverlay 'conflict'
    Check ($conflict.WaitForExit(5000) -and $conflict.ExitCode -ne 0) 'Conflicting quit shortcut aborts startup'
    $conflictError = Get-Content (Join-Path $directory 'conflict-error.log') -Raw
    Check ($conflictError.Contains('Ctrl+Alt+Q')) 'Startup reports the conflicting shortcut'
    Check ([PocSmoke]::FindWindow('MadoScreenPoc1', 'Mado Screen PoC 1') -eq [IntPtr]::Zero) 'Failed startup leaves no overlay'
    [void][PocSmoke]::UnregisterHotKey([IntPtr]::Zero, 200)
    $reserved = $false
    foreach ($key in @(0x44, 0x43)) {
        $released = [PocSmoke]::RegisterHotKey([IntPtr]::Zero, 201, 0x4003, $key)
        Check $released "Partial registration rolled back for key $key"
        if ($released) { [void][PocSmoke]::UnregisterHotKey([IntPtr]::Zero, 201) }
    }
    [ordered]@{
        checks = $checks; width = $width; height = $height
        logicalProcessors = [Environment]::ProcessorCount
        idleCpuSeconds = $cpuSeconds; idleCpuPercent = $idlePercent
        limits = 'No injected keyboard/mouse input. Real shortcuts, clicks/scrolling, focus, capture mode, sharing and multiple displays still require manual checks.'
    } | ConvertTo-Json -Depth 4 | Tee-Object (Join-Path $directory 'results.json')
    Write-Host "Logs and results: $directory"
} finally {
    if ($reserved) { [void][PocSmoke]::UnregisterHotKey([IntPtr]::Zero, 200) }
    foreach ($child in @($process, $conflict)) {
        if ($null -ne $child) {
            if (-not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
            $child.Dispose()
        }
    }
    [void][PocSmoke]::SetThreadDpiAwarenessContext($oldDpi)
}
