param([Parameter(Mandatory = $true)][string]$Exe, [switch]$PostedStroke)
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
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool RegisterHotKey(IntPtr hwnd, int id, uint mods, uint key);
    [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr hwnd, int id);
}
'@
$oldDpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$directory = Join-Path $env:TEMP ("mado-poc2-" + [Guid]::NewGuid())
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
    $lx = [int][Math]::Floor($width / 8)
    $ly = [int][Math]::Floor($height / 4)
    $rx = [int][Math]::Floor($width / 2)
    $ry = [int][Math]::Floor($height / 3)
    $beforeLine = Pixel $lx $ly
    $beforeRectangle = Pixel $rx $ry
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
    Start-Sleep -Milliseconds 300
    $ink = [Drawing.Color]::FromArgb(255, 249, 115, 22).ToArgb()
    Check ((Pixel $lx $ly) -eq $beforeLine -and (Pixel $rx $ry) -eq $beforeRectangle) 'Empty startup leaves sampled desktop pixels unchanged'
    Check ($startup.Contains('PoC 2: empty canvas')) 'Empty-canvas startup reported'
    $process.Refresh()
    $cpuStart = $process.TotalProcessorTime.TotalSeconds
    $watch = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds 30
    $process.Refresh()
    $cpuSeconds = $process.TotalProcessorTime.TotalSeconds - $cpuStart
    $idlePercent = 100 * $cpuSeconds / $watch.Elapsed.TotalSeconds / [Environment]::ProcessorCount
    Check ($idlePercent -lt 1) 'Idle CPU below 1 percent over 30 seconds'
    if ($PostedStroke) {
        # App-local messages only. These do not simulate physical input or move the cursor.
        $drawingRequested = $false
        try {
            $drawingRequested = [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(1), [IntPtr]::Zero)
            Check $drawingRequested 'Drawing toggle queued'
            $modeWait = [Diagnostics.Stopwatch]::StartNew()
            do {
                Start-Sleep -Milliseconds 50
                $style = [PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64()
                $log = [string](Get-Content (Join-Path $directory 'run-out.log') -Raw)
                $drawingReady = ($style -band 0x20) -eq 0 -and $log.Contains('Mode: drawing;')
            } while (-not $drawingReady -and -not $process.HasExited -and $modeWait.Elapsed.TotalSeconds -lt 2)
            Check $drawingReady 'Drawing mode entered within bounded wait'
            # Queue the complete stroke before checking results so the physical-state watchdog
            # does not end a synthetic drag between individual messages.
            $down = [IntPtr]::new(($ly -shl 16) -bor $lx)
            $move = [IntPtr]::new(($ly -shl 16) -bor ($lx + 10))
            $up = [IntPtr]::new(($ly -shl 16) -bor ($lx + 20))
            $queuedDown = [PocSmoke]::PostMessage($window, 0x0201, [UIntPtr]::new(1), $down)
            $queuedMove = [PocSmoke]::PostMessage($window, 0x0200, [UIntPtr]::new(1), $move)
            $queuedUp = [PocSmoke]::PostMessage($window, 0x0202, [UIntPtr]::Zero, $up)
            Check ($queuedDown -and $queuedMove -and $queuedUp) 'App-local stroke messages queued'
            Start-Sleep -Milliseconds 300
            Check ((Pixel $lx $ly) -eq $ink -and (Pixel ($lx + 10) $ly) -eq $ink -and (Pixel ($lx + 20) $ly) -eq $ink) 'Queued stroke paints start, move and final-up points'
        } finally {
            if ($drawingRequested -and -not $process.HasExited) {
                $returnQueued = [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(1), [IntPtr]::Zero)
                $modeWait = [Diagnostics.Stopwatch]::StartNew()
                do {
                    Start-Sleep -Milliseconds 50
                    $style = [PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64()
                    $log = [string](Get-Content (Join-Path $directory 'run-out.log') -Raw)
                    $restored = ($style -band 0x20) -ne 0 -and ([regex]::Matches($log, 'Mode: click-through\.')).Count -ge 2
                } while (-not $restored -and -not $process.HasExited -and $modeWait.Elapsed.TotalSeconds -lt 2)
                if (-not $returnQueued -or -not $restored) {
                    # Never leave the copied process trapping input after a failed mode check.
                    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                    throw "Drawing-mode cleanup failed. Copied overlay terminated. Logs: $directory"
                }
            }
        }
        Check ((Pixel $lx $ly) -eq $ink -and (Pixel ($lx + 20) $ly) -eq $ink) 'Returning to click-through preserves the stroke'
    }
    # Queue clear/quit directly. This intentionally does NOT prove real keyboard routing.
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(2), [IntPtr]::Zero)) 'Clear action queued'
    Start-Sleep -Milliseconds 500
    Check ((Pixel $lx $ly) -eq $beforeLine -and (Pixel $rx $ry) -eq $beforeRectangle) 'Clear leaves an empty click-through canvas'
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
        postedStroke = [bool]$PostedStroke
        limits = 'No injected keyboard/mouse input or cursor changes. Queued messages prove routing, not physical drawing, global shortcuts, focus, outside-release behavior or hardware latency. Sharing and multiple displays require manual checks.'
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
