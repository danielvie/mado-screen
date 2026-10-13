[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Run')][string]$Exe,
    [Parameter(ParameterSetName = 'Run')][switch]$PostedStroke,
    [Parameter(Mandatory = $true, ParameterSetName = 'SelfCheck')][switch]$SelfCheck
)
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
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("dwmapi.dll")] public static extern int DwmFlush();
    // 0 is inconclusive motion, 1 is transparent, -1 is a stable difference.
    public static int CompareSamples(int hiddenBefore, int visibleFirst, int visibleSecond, int hiddenAfter) {
        if (hiddenBefore != hiddenAfter || visibleFirst != visibleSecond) return 0;
        return hiddenBefore == visibleFirst ? 1 : -1;
    }
    public static bool PostDrag(IntPtr hwnd, uint thread, uint select, uint cancel, int[] points) {
        if (points.Length < 6 || points.Length % 2 != 0) throw new ArgumentException("Expected down, move and up coordinates.");
        bool queued = true;
        if (select != 0) queued &= PostThreadMessage(thread, 0x0312, new UIntPtr(select), IntPtr.Zero);
        for (int i = 0; i < points.Length; i += 2) {
            bool last = i == points.Length - 2;
            if (last && cancel != 0) queued &= PostThreadMessage(thread, 0x0312, new UIntPtr(cancel), IntPtr.Zero);
            uint message = i == 0 ? 0x0201u : last ? 0x0202u : 0x0200u;
            queued &= PostMessage(hwnd, message, new UIntPtr(last ? 0u : 1u), new IntPtr((points[i + 1] << 16) | (points[i] & 0xffff)));
        }
        return queued;
    }
}
'@
if ($SelfCheck) {
    foreach ($case in @(
        @(10, 10, 10, 10, 1),  # Empty stable canvas.
        @(10, 20, 20, 10, -1), # Stable ink differs from the desktop.
        @(10, 20, 20, 30, 0),  # Underlying desktop changed.
        @(10, 10, 20, 10, 0)   # Visible samples changed.
    )) {
        if ([PocSmoke]::CompareSamples($case[0], $case[1], $case[2], $case[3]) -ne $case[4]) {
            throw 'Sample comparison self-check failed.'
        }
    }
    Write-Host 'PASS: stable match, stable mismatch and changing-background sample classification'
    return
}
$oldDpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$directory = Join-Path $env:TEMP ("mado-poc4-" + [Guid]::NewGuid())
New-Item -ItemType Directory $directory | Out-Null
$copy = Join-Path $directory 'overlay-smoke.exe'
Copy-Item (Resolve-Path $Exe).Path $copy
$checks = [Collections.Generic.List[string]]::new()
$skipped = [Collections.Generic.List[string]]::new()
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
function ProbeToolKeys([bool]$ShouldBeAvailable) {
    foreach ($key in @(0x50, 0x4c, 0x52)) {
        $available = [PocSmoke]::RegisterHotKey([IntPtr]::Zero, 201, 0x4000, $key)
        if ($available) { [void][PocSmoke]::UnregisterHotKey([IntPtr]::Zero, 201) }
        Check ($available -eq $ShouldBeAvailable) "Tool key $key availability=$ShouldBeAvailable"
    }
}
function SetCanvasVisibility([bool]$Visible) {
    [uint32]$owner = 0
    [void][PocSmoke]::GetWindowThreadProcessId($window, [ref]$owner)
    if ($process.HasExited -or $owner -ne $process.Id) { throw 'Refusing to change a window not owned by the copied process.' }
    $command = if ($Visible) { 4 } else { 0 } # SW_SHOWNOACTIVATE / SW_HIDE. Never change focus.
    [void][PocSmoke]::ShowWindow($window, $command)
    [Runtime.InteropServices.Marshal]::ThrowExceptionForHR([PocSmoke]::DwmFlush())
    if ([PocSmoke]::IsWindowVisible($window) -ne $Visible) { throw 'Overlay visibility did not change as requested.' }
}
function CheckEmptyCanvas([string]$Name) {
    $matched = @($false, $false)
    $mismatches = @(0, 0)
    $points = @(@($lx, $ly), @($rx, $ry))
    # Bracket visible samples with fresh hidden references. Motion is not evidence of ink.
    for ($attempt = 0; $attempt -lt 5 -and $matched.Contains($false); $attempt++) {
        try {
            SetCanvasVisibility $false
            $before = @(foreach ($point in $points) { Pixel $point[0] $point[1] })
            SetCanvasVisibility $true
            $first = @(foreach ($point in $points) { Pixel $point[0] $point[1] })
            [Runtime.InteropServices.Marshal]::ThrowExceptionForHR([PocSmoke]::DwmFlush())
            $second = @(foreach ($point in $points) { Pixel $point[0] $point[1] })
            SetCanvasVisibility $false
            $after = @(foreach ($point in $points) { Pixel $point[0] $point[1] })
        } finally { SetCanvasVisibility $true }
        for ($i = 0; $i -lt $points.Count; $i++) {
            $comparison = [PocSmoke]::CompareSamples($before[$i], $first[$i], $second[$i], $after[$i])
            if ($comparison -eq 1) { $matched[$i] = $true }
            if ($comparison -eq -1) { $mismatches[$i]++ }
        }
        if ($matched.Contains($false)) { Start-Sleep -Milliseconds 50 }
    }
    for ($i = 0; $i -lt $points.Count; $i++) {
        # Fail only persistent stable differences; never claim a moving point passed.
        if ($matched[$i]) {
            if ($matched.Contains($false)) { Check $true "$Name at stable sample $i" }
        } elseif ($mismatches[$i] -eq 5) {
            Check $false "$Name at sample $i has a persistent stable difference"
        } else {
            $reason = "SKIPPED/UNTESTED: $Name at sample ${i}, samples changed across 5 attempts"
            $skipped.Add($reason)
            Write-Warning $reason
        }
    }
    if (-not $matched.Contains($false)) { Check $true $Name }
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
    CheckEmptyCanvas 'Empty startup leaves sampled desktop pixels unchanged'
    Check ($startup.Contains('PoC 4:')) 'PoC 4 startup reported'
    ProbeToolKeys $true
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
            ProbeToolKeys $false
            # Queue the complete stroke before checking results so the physical-state watchdog
            # does not end a synthetic drag between individual messages.
            Check ([PocSmoke]::PostDrag($window, $thread, 0, 0, [int[]]@($lx, $ly, ($lx + 10), $ly, ($lx + 20), $ly))) 'App-local stroke messages queued'
            Start-Sleep -Milliseconds 300
            Check ((Pixel $lx $ly) -eq $ink -and (Pixel ($lx + 10) $ly) -eq $ink -and (Pixel ($lx + 20) $ly) -eq $ink) 'Queued pen paints start, move and final-up points'
            Check ([PocSmoke]::PostDrag($window, $thread, 5, 0, [int[]]@(($lx + 60), $ly, ($lx + 80), ($ly + 40), ($lx + 140), $ly, ($lx + 140), $ly))) 'Line selection, preview and release queued'
            Start-Sleep -Milliseconds 300
            Check ((Pixel ($lx + 100) $ly) -eq $ink -and (Pixel ($lx + 140) $ly) -eq $ink) 'Straight line committed'
            Check ((Pixel ($lx + 70) ($ly + 20)) -ne $ink) 'Replaced line preview leaves no trail'
            Check ([PocSmoke]::PostDrag($window, $thread, 6, 0, [int[]]@(($lx + 240), ($ly + 120), ($lx + 160), ($ly + 60), ($lx + 160), ($ly + 60)))) 'Rectangle selection and reverse-direction rectangle queued'
            Start-Sleep -Milliseconds 300
            Check ((Pixel ($lx + 160) ($ly + 60)) -eq $ink -and (Pixel ($lx + 240) ($ly + 60)) -eq $ink -and (Pixel ($lx + 160) ($ly + 120)) -eq $ink -and (Pixel ($lx + 240) ($ly + 120)) -eq $ink) 'Rectangle has all four corners'
            Check ((Pixel ($lx + 200) ($ly + 90)) -ne $ink) 'Rectangle interior remains unfilled'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(11), [IntPtr]::Zero) -and [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(10), [IntPtr]::Zero)) 'Blue color and thicker brush queued'
            Check ([PocSmoke]::PostDrag($window, $thread, 4, 0, [int[]]@(($lx + 500), $ly, ($lx + 520), $ly, ($lx + 540), $ly))) 'Styled pen queued'
            Start-Sleep -Milliseconds 200
            $blue = [Drawing.Color]::FromArgb(255, 59, 130, 246).ToArgb()
            Check ((Pixel ($lx + 520) $ly) -eq $blue -and (Pixel ($lx + 520) ($ly + 3)) -eq $blue -and (Pixel $lx $ly) -eq $ink) 'New stroke is blue and seven pixels thick; old ink retains its style'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(7), [IntPtr]::Zero)) 'Undo queued'
            Start-Sleep -Milliseconds 200
            Check ((Pixel ($lx + 520) $ly) -ne $blue -and (Pixel $lx $ly) -eq $ink) 'Undo removes only the last annotation'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(8), [IntPtr]::Zero)) 'Redo queued'
            Start-Sleep -Milliseconds 200
            Check ((Pixel ($lx + 520) $ly) -eq $blue) 'Redo restores color and thickness'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(2), [IntPtr]::Zero)) 'Undoable clear queued'
            Start-Sleep -Milliseconds 200
            Check ((Pixel $lx $ly) -ne $ink -and (Pixel ($lx + 520) $ly) -ne $blue) 'Clear removes mixed styled annotations'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(7), [IntPtr]::Zero)) 'Undo clear queued'
            Start-Sleep -Milliseconds 200
            Check ((Pixel $lx $ly) -eq $ink -and (Pixel ($lx + 520) $ly) -eq $blue) 'Undo clear restores the mixed styled scene'
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(12), [IntPtr]::Zero) -and [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(9), [IntPtr]::Zero)) 'Restore orange five-pixel style'
            if ($startup.Contains('stage 2') -or $startup.Contains('stage 3')) {
                Check ([PocSmoke]::PostDrag($window, $thread, 16, 0, [int[]]@(($lx + 600), $ly, ($lx + 640), $ly, ($lx + 680), $ly))) 'Arrow gesture queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 640) $ly) -eq $ink -and (Pixel ($lx + 680) $ly) -eq $ink) 'Arrow shaft and tip rendered'
                Check ([PocSmoke]::PostDrag($window, $thread, 17, 0, [int[]]@(($lx + 800), ($ly + 120), ($lx + 720), ($ly + 60), ($lx + 720), ($ly + 60)))) 'Reverse-direction ellipse queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 760) ($ly + 60)) -eq $ink -and (Pixel ($lx + 800) ($ly + 90)) -eq $ink -and (Pixel ($lx + 760) ($ly + 90)) -ne $ink) 'Ellipse outline bounds rendered with empty interior'
                Check ([PocSmoke]::PostDrag($window, $thread, 18, 0, [int[]]@(($lx + 520), $ly, ($lx + 520), $ly, ($lx + 520), $ly))) 'Whole-annotation eraser queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 500) $ly) -ne $blue -and (Pixel ($lx + 540) $ly) -ne $blue -and (Pixel $lx $ly) -eq $ink) 'Eraser removes the whole touched annotation only'
                Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(7), [IntPtr]::Zero)) 'Undo erasing queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 500) $ly) -eq $blue -and (Pixel ($lx + 540) $ly) -eq $blue) 'Undo restores entire erased annotation and style'
                Check ([PocSmoke]::PostDrag($window, $thread, 18, 0, [int[]]@(($lx + 760), ($ly + 90), ($lx + 760), ($ly + 90), ($lx + 760), ($ly + 90)))) 'Eraser gesture inside ellipse queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 760) ($ly + 60)) -eq $ink -and (Pixel ($lx + 800) ($ly + 90)) -eq $ink) 'Eraser ignores empty shape interiors'
            }
            # Queue cancellation in the same burst, before the physical-release watchdog.
            Check ([PocSmoke]::PostDrag($window, $thread, 5, 6, [int[]]@(($lx + 300), $ly, ($lx + 360), $ly, ($lx + 360), $ly))) 'Line selection, unfinished line, tool cancellation and stale release queued'
            Start-Sleep -Milliseconds 300
            Check ((Pixel ($lx + 330) $ly) -ne $ink) 'Tool change discards unfinished line'
            Check ([PocSmoke]::PostDrag($window, $thread, 0, 1, [int[]]@(($lx + 400), $ly, ($lx + 460), ($ly + 40), ($lx + 460), ($ly + 40)))) 'Unfinished rectangle, mode cancellation and stale shape release queued'
            Start-Sleep -Milliseconds 300
            $drawingRequested = $false
            Check (([PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64() -band 0x20) -ne 0) 'Mode change restores click-through'
            Check ((Pixel ($lx + 430) $ly) -ne $ink) 'Mode change discards unfinished rectangle'
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
        Check ((Pixel $lx $ly) -eq $ink -and (Pixel ($lx + 20) $ly) -eq $ink -and (Pixel ($lx + 100) $ly) -eq $ink -and (Pixel ($lx + 240) ($ly + 120)) -eq $ink) 'Click-through preserves committed pen, line and rectangle'
        ProbeToolKeys $true
        # Reserve R only for the bounded registration-conflict attempt.
        $toolReserved = [PocSmoke]::RegisterHotKey([IntPtr]::Zero, 202, 0x4000, 0x52)
        Check $toolReserved 'Test reserves bare R'
        try {
            Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(1), [IntPtr]::Zero)) 'Conflicting drawing-mode entry queued'
            Start-Sleep -Milliseconds 300
            $failure = [string](Get-Content (Join-Path $directory 'run-error.log') -Raw)
            Check (-not $process.HasExited -and $failure.Contains('Cannot enter drawing mode:') -and $failure.Contains('Could not register R')) 'Tool conflict reports R without exiting'
            Check (([PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64() -band 0x20) -ne 0 -and (Pixel $lx $ly) -eq $ink) 'Tool conflict preserves ink and click-through'
        } finally {
            if ($toolReserved) { [void][PocSmoke]::UnregisterHotKey([IntPtr]::Zero, 202) }
        }
        ProbeToolKeys $true
    }
    # Queue clear/quit directly. This intentionally does NOT prove real keyboard routing.
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(2), [IntPtr]::Zero)) 'Clear action queued'
    Start-Sleep -Milliseconds 500
    CheckEmptyCanvas 'Clear leaves an empty click-through canvas'
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
        checks = $checks; skipped = $skipped; width = $width; height = $height
        logicalProcessors = [Environment]::ProcessorCount
        idleCpuSeconds = $cpuSeconds; idleCpuPercent = $idlePercent
        postedStroke = [bool]$PostedStroke
        limits = 'Empty-canvas checks report unstable points as skipped/untested, not passed. No injected keyboard/mouse input or cursor changes. Queued messages prove routing, not physical drawing, global shortcuts, focus, outside-release behavior or hardware latency. Sharing and multiple displays require manual checks.'
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
