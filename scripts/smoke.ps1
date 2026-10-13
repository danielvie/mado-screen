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
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class PocSmoke {
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    // PowerShell converts $null to an empty string; pass the null class from C#.
    public static IntPtr FindWindowByTitle(string title) { return FindWindow(null, title); }
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd, uint command);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] private struct MonitorInfo {
        public int Size;
        public Rect Bounds, Work;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string Device;
    }
    public sealed class Monitor {
        public string Device;
        public Rect Bounds;
        public bool Primary;
    }
    private delegate bool MonitorCallback(IntPtr monitor, IntPtr dc, ref Rect bounds, IntPtr data);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonitorCallback callback, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] private static extern bool GetMonitorInfoW(IntPtr monitor, ref MonitorInfo info);
    public static Monitor[] PhysicalMonitors() {
        IntPtr previous = SetThreadDpiAwarenessContext(new IntPtr(-4));
        if (previous == IntPtr.Zero) throw new Win32Exception("Cannot enumerate physical monitor bounds without per-monitor DPI awareness.");
        try {
            var monitors = new List<Monitor>();
            int error = 0;
            MonitorCallback callback = delegate(IntPtr monitor, IntPtr dc, ref Rect bounds, IntPtr data) {
                var info = new MonitorInfo { Size = Marshal.SizeOf(typeof(MonitorInfo)) };
                if (!GetMonitorInfoW(monitor, ref info)) { error = Marshal.GetLastWin32Error(); return false; }
                monitors.Add(new Monitor { Device = info.Device, Bounds = info.Bounds, Primary = (info.Flags & 1) != 0 });
                return true;
            };
            bool enumerated = EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, callback, IntPtr.Zero);
            if (!enumerated) throw new Win32Exception(error != 0 ? error : Marshal.GetLastWin32Error());
            return monitors.ToArray();
        } finally { SetThreadDpiAwarenessContext(previous); }
    }
    public static int ToScreen(int local, int origin) { return checked(local + origin); }
    public static int ToCanvas(int screen, int origin) { return checked(screen - origin); }
    // Axis-aligned geometry keeps exact pixel assertions independent of antialiasing.
    public static int[] AdjacentLine(Rect a, Rect b) {
        if (b.Right == a.Left || b.Bottom == a.Top) { Rect swap = a; a = b; b = swap; }
        if (a.Right == b.Left) {
            int low = Math.Max(a.Top, b.Top) + 32, high = Math.Min(a.Bottom, b.Bottom) - 32;
            int reach = Math.Min(80, Math.Min(a.Right - a.Left, b.Right - b.Left) - 16);
            if (low > high || reach < 16) return null;
            int y = low + (high - low) / 2;
            return new int[] { a.Right - reach, y, a.Right + reach, y };
        }
        if (a.Bottom == b.Top) {
            int low = Math.Max(a.Left, b.Left) + 32, high = Math.Min(a.Right, b.Right) - 32;
            int reach = Math.Min(80, Math.Min(a.Bottom - a.Top, b.Bottom - b.Top) - 16);
            if (low > high || reach < 16) return null;
            int x = low + (high - low) / 2;
            return new int[] { x, a.Bottom - reach, x, a.Bottom + reach };
        }
        return null;
    }
    public static int[] LineSamples(int[] line) {
        return new int[] { line[0], line[1], line[2], line[3],
            line[0] + (line[2] - line[0]) / 4, line[1] + (line[3] - line[1]) / 4,
            line[0] + (line[2] - line[0]) * 3 / 4, line[1] + (line[3] - line[1]) * 3 / 4 };
    }
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
    foreach ($case in @(
        @(120, 240, 0, 0, 120, 240),
        @(120, 240, -1920, -1080, -1800, -840),
        @(1920, 1080, -1920, -1080, 0, 0),
        @(10, 20, 2560, -1440, 2570, -1420)
    )) {
        if ([PocSmoke]::ToScreen($case[0], $case[2]) -ne $case[4] -or
            [PocSmoke]::ToScreen($case[1], $case[3]) -ne $case[5] -or
            [PocSmoke]::ToCanvas($case[4], $case[2]) -ne $case[0] -or
            [PocSmoke]::ToCanvas($case[5], $case[3]) -ne $case[1]) { throw 'Coordinate translation self-check failed.' }
    }
    $left = [PocSmoke+Rect]@{ Left = -1920; Top = 0; Right = 0; Bottom = 1080 }
    $primary = [PocSmoke+Rect]@{ Left = 0; Top = 0; Right = 1920; Bottom = 1080 }
    $above = [PocSmoke+Rect]@{ Left = 0; Top = -1080; Right = 1920; Bottom = 0 }
    $gap = [PocSmoke+Rect]@{ Left = 1921; Top = 0; Right = 3841; Bottom = 1080 }
    $corner = [PocSmoke+Rect]@{ Left = 1920; Top = 1080; Right = 3840; Bottom = 2160 }
    foreach ($case in @(
        @($left, $primary, '-80,540,80,540'),
        @($primary, $left, '-80,540,80,540'),
        @($above, $primary, '960,-80,960,80'),
        @($primary, $above, '960,-80,960,80')
    )) {
        $line = [PocSmoke]::AdjacentLine($case[0], $case[1])
        if (($line -join ',') -ne $case[2]) { throw 'Adjacent-monitor seam self-check failed.' }
        $samples = [PocSmoke]::LineSamples($line)
        if ($samples.Count -ne 8 -or ($samples[0..3] -join ',') -ne $case[2]) { throw 'Line endpoint sampling self-check failed.' }
    }
    if ($null -ne [PocSmoke]::AdjacentLine($primary, $gap) -or $null -ne [PocSmoke]::AdjacentLine($primary, $corner)) {
        throw 'Nonadjacent monitors must not produce a seam test.'
    }
    if (([PocSmoke]::LineSamples([int[]]@(-80, 540, 80, 540)) -join ',') -ne '-80,540,80,540,-40,540,40,540' -or
        ([PocSmoke]::LineSamples([int[]]@(960, -80, 960, 80)) -join ',') -ne '960,-80,960,80,960,-40,960,40') {
        throw 'Interpolation sampling self-check failed.'
    }
    Write-Host 'PASS: stable match, stable mismatch and changing-background sample classification'
    Write-Host 'PASS: canvas/screen translation with negative left/above origins and round trips'
    Write-Host 'PASS: horizontal/vertical adjacent seams, reversed order, gap/corner rejection and interpolation samples'
    return
}
$oldDpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$directory = Join-Path $env:TEMP ("mado-poc5-" + [Guid]::NewGuid())
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
    $dpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    try {
        if ($dpi -eq [IntPtr]::Zero) { throw 'Cannot sample physical pixels without per-monitor DPI awareness.' }
        $graphics.CopyFromScreen([PocSmoke]::ToScreen($X, $virtualLeft), [PocSmoke]::ToScreen($Y, $virtualTop), 0, 0, [Drawing.Size]::new(1, 1))
        return $bitmap.GetPixel(0, 0).ToArgb()
    } finally {
        if ($dpi -ne [IntPtr]::Zero) { [void][PocSmoke]::SetThreadDpiAwarenessContext($dpi) }
        $graphics.Dispose(); $bitmap.Dispose()
    }
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
function CheckOwnedCanvas {
    [uint32]$owner = 0
    [void][PocSmoke]::GetWindowThreadProcessId($window, [ref]$owner)
    if ($process.HasExited -or $owner -ne $process.Id) { throw 'Refusing to post to or inspect a window not owned by the copied process.' }
}
function CheckVirtualBounds([string]$Name) {
    CheckOwnedCanvas
    $dpi = [PocSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    try {
        if ($dpi -eq [IntPtr]::Zero) { throw 'Cannot check physical window bounds without per-monitor DPI awareness.' }
        $bounds = [PocSmoke+Rect]::new()
        Check ([PocSmoke]::GetWindowRect($window, [ref]$bounds) -and
            $bounds.Left -eq $virtualLeft -and $bounds.Top -eq $virtualTop -and
            $bounds.Right -eq ($virtualLeft + $virtualWidth) -and $bounds.Bottom -eq ($virtualTop + $virtualHeight)) $Name
    } finally {
        if ($dpi -ne [IntPtr]::Zero) { [void][PocSmoke]::SetThreadDpiAwarenessContext($dpi) }
    }
}
function CanvasPoints([int[]]$ScreenPoints) {
    for ($i = 0; $i -lt $ScreenPoints.Count; $i += 2) {
        [PocSmoke]::ToCanvas($ScreenPoints[$i], $virtualLeft)
        [PocSmoke]::ToCanvas($ScreenPoints[$i + 1], $virtualTop)
    }
}
function CheckInk([int[]]$Points, [string]$Name) {
    $painted = $true
    for ($i = 0; $i -lt $Points.Count; $i += 2) {
        if ((Pixel $Points[$i] $Points[$i + 1]) -ne $ink) { $painted = $false }
    }
    Check $painted $Name
}
function PostAction([uint32]$Action, [string]$Name) {
    CheckOwnedCanvas
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new($Action), [IntPtr]::Zero)) $Name
    Start-Sleep -Milliseconds 200
}
function PostLine([int[]]$Line, [string]$Name) {
    CheckOwnedCanvas
    # One down, one endpoint preview, one up. Interior samples were never posted.
    Check ([PocSmoke]::PostDrag($window, $thread, 5, 0, [int[]]@($Line[0], $Line[1], $Line[2], $Line[3], $Line[2], $Line[3]))) $Name
    Start-Sleep -Milliseconds 300
}
function CheckLineHistory([int[]]$Line, [string]$Name, [int[]]$Earlier = @()) {
    $samples = [PocSmoke]::LineSamples($Line)
    PostLine $Line "$Name posted line queued"
    CheckInk $samples "$Name endpoints and unposted interpolation render"
    CheckVirtualBounds "$Name drawing retains exact virtual window bounds"
    PostAction 7 "$Name one undo queued"
    CheckEmptyCanvas "$Name one undo removes the entire line" $samples
    if ($Earlier.Count) { CheckInk $Earlier "$Name undo preserves earlier monitors' annotations" }
    PostAction 8 "$Name one redo queued"
    CheckInk $samples "$Name one redo restores the entire line"
    CheckOwnedCanvas
    Check ([PocSmoke]::PostDrag($window, $thread, 18, 0, [int[]]@($samples[4], $samples[5], $samples[4], $samples[5], $samples[4], $samples[5]))) "$Name posted eraser queued on one interior point"
    Start-Sleep -Milliseconds 300
    CheckEmptyCanvas "$Name one erase removes the entire line" $samples
    if ($Earlier.Count) { CheckInk $Earlier "$Name erase preserves earlier monitors' annotations" }
    PostAction 7 "$Name undo erase queued"
    CheckInk $samples "$Name undo erase restores both endpoints and interpolation"
}
function SetPostedDrawingMode([bool]$Drawing) {
    PostAction 1 "Posted drawing mode toggle queued; requested drawing=$Drawing"
    $wait = [Diagnostics.Stopwatch]::StartNew()
    do {
        CheckOwnedCanvas
        $active = ([PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64() -band 0x20) -eq 0
        if ($active -eq $Drawing) { break }
        Start-Sleep -Milliseconds 50
    } while ($wait.Elapsed.TotalSeconds -lt 2)
    Check ($active -eq $Drawing) "Posted drawing=$Drawing reached within bounded wait"
}
function CheckEmptyCanvas([string]$Name, [int[]]$Points = @($lx, $ly, $rx, $ry)) {
    $matched = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { $false })
    $mismatches = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { 0 })
    # Bracket visible samples with fresh hidden references. Motion is not evidence of ink.
    for ($attempt = 0; $attempt -lt 5 -and $matched.Contains($false); $attempt++) {
        try {
            SetCanvasVisibility $false
            $before = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { Pixel $Points[$i] $Points[$i + 1] })
            SetCanvasVisibility $true
            $first = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { Pixel $Points[$i] $Points[$i + 1] })
            [Runtime.InteropServices.Marshal]::ThrowExceptionForHR([PocSmoke]::DwmFlush())
            $second = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { Pixel $Points[$i] $Points[$i + 1] })
            SetCanvasVisibility $false
            $after = @(for ($i = 0; $i -lt $Points.Count; $i += 2) { Pixel $Points[$i] $Points[$i + 1] })
        } finally { SetCanvasVisibility $true }
        for ($i = 0; $i -lt $matched.Count; $i++) {
            $comparison = [PocSmoke]::CompareSamples($before[$i], $first[$i], $second[$i], $after[$i])
            if ($comparison -eq 1) { $matched[$i] = $true }
            if ($comparison -eq -1) { $mismatches[$i]++ }
        }
        if ($matched.Contains($false)) { Start-Sleep -Milliseconds 50 }
    }
    for ($i = 0; $i -lt $matched.Count; $i++) {
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
    Check ($oldDpi -ne [IntPtr]::Zero) 'Smoke thread uses per-monitor DPI awareness'
    $virtualLeft = [PocSmoke]::GetSystemMetrics(76) # SM_XVIRTUALSCREEN
    $virtualTop = [PocSmoke]::GetSystemMetrics(77) # SM_YVIRTUALSCREEN
    $virtualWidth = [PocSmoke]::GetSystemMetrics(78) # SM_CXVIRTUALSCREEN
    $virtualHeight = [PocSmoke]::GetSystemMetrics(79) # SM_CYVIRTUALSCREEN
    $monitors = @([PocSmoke]::PhysicalMonitors())
    Check ($monitors.Count -gt 0) 'DPI-aware EnumDisplayMonitors/GetMonitorInfo reports physical monitor bounds'
    $primary = @($monitors | Where-Object { $_.Primary })
    Check ($primary.Count -eq 1) 'One primary monitor enumerated'
    Check ($primary[0].Bounds.Left -eq 0 -and $primary[0].Bounds.Top -eq 0) 'Primary display origin preserves legacy gesture placement'
    $width = $primary[0].Bounds.Right - $primary[0].Bounds.Left
    $height = $primary[0].Bounds.Bottom - $primary[0].Bounds.Top
    # Retain legacy physical primary-display gestures; posted coordinates are canvas-local.
    $lx = [int][Math]::Floor($width / 8) - $virtualLeft
    $ly = [int][Math]::Floor($height / 4) - $virtualTop
    $rx = [int][Math]::Floor($width / 2) - $virtualLeft
    $ry = [int][Math]::Floor($height / 3) - $virtualTop
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
    CheckVirtualBounds 'Startup overlay spans the exact physical virtual desktop'
    $style = [PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64()
    Check (($style -band 0x080800a8) -eq 0x080800a8) ('Topmost layered click-through nonactivating tool window styles: 0x{0:x}' -f $style)
    Start-Sleep -Milliseconds 300
    $ink = [Drawing.Color]::FromArgb(255, 249, 115, 22).ToArgb()
    if ($startup.Contains('stage 3')) {
        $statusWindow = [PocSmoke]::FindWindowByTitle('Mado status')
        [uint32]$statusOwner = 0
        [void][PocSmoke]::GetWindowThreadProcessId($statusWindow, [ref]$statusOwner)
        Check ([PocSmoke]::IsWindowVisible($statusWindow) -and $statusOwner -eq $process.Id -and [PocSmoke]::GetWindow($statusWindow, 4) -eq $window) 'Owned status indicator is visible before canvas sampling'
    }
    CheckEmptyCanvas 'Empty startup leaves sampled desktop pixels unchanged'
    Check ($startup.Contains('PoC 5: stage 3')) 'PoC 5 stage 3 startup reported'
    ProbeToolKeys $true
    if ($startup.Contains('stage 3')) {
        $statusWindow = [PocSmoke]::FindWindowByTitle('Mado status')
        [uint32]$statusOwner = 0
        [void][PocSmoke]::GetWindowThreadProcessId($statusWindow, [ref]$statusOwner)
        Check ([PocSmoke]::IsWindowVisible($statusWindow) -and $statusOwner -eq $process.Id -and [PocSmoke]::GetWindow($statusWindow, 4) -eq $window) 'Owned status indicator is visible'
        $statusStyle = [PocSmoke]::GetWindowLongPtrW($statusWindow, -20).ToInt64()
        Check (($statusStyle -band 0x080800a8) -eq 0x080800a8) 'Status indicator is topmost layered click-through and nonactivating'
        $statusBounds = [PocSmoke+Rect]::new()
        Check ([PocSmoke]::GetWindowRect($statusWindow, [ref]$statusBounds)) 'Status bounds available for visual evidence'
        $statusSize = [Drawing.Size]::new(($statusBounds.Right - $statusBounds.Left), ($statusBounds.Bottom - $statusBounds.Top))
        $statusBitmap = [Drawing.Bitmap]::new($statusSize.Width, $statusSize.Height)
        $statusGraphics = [Drawing.Graphics]::FromImage($statusBitmap)
        try {
            $statusGraphics.CopyFromScreen($statusBounds.Left, $statusBounds.Top, 0, 0, $statusSize)
            $statusBitmap.Save((Join-Path $directory 'status.png'))
        } finally { $statusGraphics.Dispose(); $statusBitmap.Dispose() }
    }
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
            CheckVirtualBounds 'Legacy pen upload retains exact virtual window bounds'
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
                Check ((Pixel ($lx + 600) $ly) -eq $ink -and (Pixel ($lx + 640) $ly) -eq $ink -and (Pixel ($lx + 639) ($ly + 20)) -eq $ink -and (Pixel ($lx + 639) ($ly - 20)) -eq $ink) 'Arrow head stays at press point with larger separate wings'
                Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(10), [IntPtr]::Zero) -and [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(10), [IntPtr]::Zero)) 'Thicker arrow style queued'
                Check ([PocSmoke]::PostDrag($window, $thread, 16, 0, [int[]]@(($lx + 600), ($ly + 220), ($lx + 680), ($ly + 220), ($lx + 800), ($ly + 220)))) 'Thicker head-first arrow queued'
                Start-Sleep -Milliseconds 200
                Check ((Pixel ($lx + 651) ($ly + 246)) -eq $ink -and (Pixel ($lx + 651) ($ly + 194)) -eq $ink -and (Pixel ($lx + 800) ($ly + 246)) -ne $ink) 'Wider stroke enlarges both head wings without moving them to the tail'
                Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(9), [IntPtr]::Zero) -and [PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(9), [IntPtr]::Zero)) 'Restore five-pixel width after arrow checks'
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
            if ($startup.Contains('stage 3')) {
                # Value-only app event checks color routing without opening/focusing the popup.
                Check ([PocSmoke]::PostMessage($window, 0x8028, [UIntPtr]::new([uint64]4279383126), [IntPtr]::new(1))) 'Arbitrary RGB color result queued'
                Check ([PocSmoke]::PostDrag($window, $thread, 4, 0, [int[]]@(($lx + 860), $ly, ($lx + 880), $ly, ($lx + 900), $ly))) 'Custom RGB annotation queued'
                Start-Sleep -Milliseconds 200
                $custom = [Drawing.Color]::FromArgb(255, 18, 52, 86).ToArgb()
                Check ((Pixel ($lx + 880) $ly) -eq $custom -and (Pixel $lx $ly) -eq $ink) 'Picker result routing applies arbitrary RGB without recoloring old ink'
                Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(12), [IntPtr]::Zero)) 'Restore orange after custom RGB'
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
    $monitorCases = @()
    $allMonitorSamples = @()
    foreach ($monitor in $monitors) {
        $bounds = $monitor.Bounds
        $monitorWidth = $bounds.Right - $bounds.Left
        $monitorHeight = $bounds.Bottom - $bounds.Top
        if ($monitorWidth -lt 160 -or $monitorHeight -lt 128) {
            $reason = "SKIPPED/UNTESTED: $($monitor.Device) has insufficient room for isolated posted geometry away from status"
            $skipped.Add($reason); Write-Warning $reason
            continue
        }
        # Lower interior, away from status/taskbars. Legacy assertions already ran.
        $x = $bounds.Left + [int][Math]::Floor($monitorWidth / 4)
        $y = $bounds.Top + [int][Math]::Floor($monitorHeight * 3 / 4)
        $line = [int[]](CanvasPoints ([int[]]@($x, $y, ($x + 80), $y)))
        $samples = [PocSmoke]::LineSamples($line)
        $monitorCases += [pscustomobject]@{ Device = $monitor.Device; Line = $line; Samples = $samples }
        $allMonitorSamples += $samples
    }
    $seam = $null
    $seamDescription = $null
    for ($i = 0; $i -lt $monitors.Count -and $null -eq $seam; $i++) {
        for ($j = $i + 1; $j -lt $monitors.Count -and $null -eq $seam; $j++) {
            $screenLine = [PocSmoke]::AdjacentLine($monitors[$i].Bounds, $monitors[$j].Bounds)
            if ($null -eq $screenLine) { continue }
            # Reject geometry covered by the owned status indicator, including brush margin.
            if ($null -ne $statusBounds -and
                [Math]::Min($screenLine[0], $screenLine[2]) -le ($statusBounds.Right + 16) -and
                [Math]::Max($screenLine[0], $screenLine[2]) -ge ($statusBounds.Left - 16) -and
                [Math]::Min($screenLine[1], $screenLine[3]) -le ($statusBounds.Bottom + 16) -and
                [Math]::Max($screenLine[1], $screenLine[3]) -ge ($statusBounds.Top - 16)) { continue }
            $seam = [int[]](CanvasPoints $screenLine)
            $orientation = if ($screenLine[1] -eq $screenLine[3]) { 'horizontal' } else { 'vertical' }
            $seamDescription = "$orientation seam between $($monitors[$i].Device) and $($monitors[$j].Device)"
            $allMonitorSamples += [PocSmoke]::LineSamples($seam)
        }
    }
    if ($null -eq $seam) {
        $reason = 'SKIPPED/UNTESTED: no suitable horizontally/vertically adjacent monitor pair with room away from status for a seam-crossing line'
        $skipped.Add($reason); Write-Warning $reason
    }
    if ($PostedStroke) {
        # Clear only after legacy ink, click-through and conflict assertions have finished.
        PostAction 2 'Clear legacy setup before isolated posted multiple-monitor cases'
        CheckEmptyCanvas 'Legacy setup cleared before isolated multiple-monitor cases'
        if ($allMonitorSamples.Count) { CheckEmptyCanvas 'Isolated monitor sample locations initially empty' $allMonitorSamples }
        try {
            SetPostedDrawingMode $true
            ProbeToolKeys $false
            if ($null -ne $seam) {
                CheckLineHistory $seam "Posted $seamDescription"
                SetPostedDrawingMode $false
                CheckInk ([PocSmoke]::LineSamples($seam)) 'Click-through preserves both screens of the posted seam line'
                SetPostedDrawingMode $true
                PostAction 2 'Clear isolated seam setup before per-monitor cases'
                CheckEmptyCanvas 'Clear removes both screens of the seam line' ([PocSmoke]::LineSamples($seam))
            }
            $earlier = @()
            foreach ($case in $monitorCases) {
                CheckLineHistory $case.Line "Posted monitor $($case.Device)" $earlier
                $earlier += $case.Samples
            }
            CheckVirtualBounds 'Per-monitor history and uploads retain exact virtual window bounds'
        } finally {
            if (-not $process.HasExited -and ([PocSmoke]::GetWindowLongPtrW($window, -20).ToInt64() -band 0x20) -eq 0) {
                try { SetPostedDrawingMode $false } catch {
                    # Terminate only this copied child if posted mode cleanup failed.
                    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                    throw
                }
            }
        }
        foreach ($case in $monitorCases) {
            CheckInk $case.Samples "Click-through preserves posted ink on $($case.Device)"
        }
        ProbeToolKeys $true
    } else {
        $reason = 'SKIPPED/UNTESTED: per-monitor draw/undo/redo/erase and seam routing require -PostedStroke'
        $skipped.Add($reason); Write-Warning $reason
    }
    # Queue clear/quit directly. This intentionally does NOT prove real keyboard routing.
    Check ([PocSmoke]::PostThreadMessage($thread, 0x0312, [UIntPtr]::new(2), [IntPtr]::Zero)) 'Clear action queued'
    Start-Sleep -Milliseconds 500
    CheckEmptyCanvas 'Clear leaves an empty click-through canvas'
    if ($allMonitorSamples.Count) { CheckEmptyCanvas 'Clear leaves all tested monitor and seam locations empty' $allMonitorSamples }
    CheckVirtualBounds 'Final clear retains exact virtual window bounds'
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
        virtualDesktop = [ordered]@{ left = $virtualLeft; top = $virtualTop; width = $virtualWidth; height = $virtualHeight }
        monitorBoundsSource = 'DPI-aware EnumDisplayMonitors/GetMonitorInfoW, physical screen pixels'
        monitors = @(foreach ($monitor in $monitors) {
            [ordered]@{ device = $monitor.Device; primary = $monitor.Primary; physicalBounds = $monitor.Bounds }
        })
        postedMonitorCases = @(if ($PostedStroke) { $monitorCases | ForEach-Object { $_.Device } })
        adjacentSeam = $seamDescription
        adjacentSeamRouting = if ($PostedStroke -and $null -ne $seam) { 'Posted endpoint/interpolation, undo/redo/erase and click-through checks executed; see checks/skipped for stable sample outcomes' } else { 'SKIPPED/UNTESTED' }
        logicalProcessors = [Environment]::ProcessorCount
        idleCpuSeconds = $cpuSeconds; idleCpuPercent = $idlePercent
        postedStroke = [bool]$PostedStroke
        evidence = 'App-local posted-message routing and sampled rendering only; no physical-input claim.'
        manualBehavior = 'UNTESTED: real strokes on each monitor/across seams, boundary capture/outside release, click-through typing, global shortcuts, picker placement/focus/scaling and hardware latency.'
        mixedDpi = 'UNTESTED: posted physical-pixel geometry does not establish mixed-DPI physical-input alignment or picker scaling.'
        limits = 'Empty-canvas checks sample tested geometry, not every pixel; unstable points are skipped/untested, not passed. No injected keyboard/mouse input, cursor movement or focus changes. Only windows owned by the copied child are modified; a pre-existing overlay aborts the run. Restart after display topology/scaling changes. Mirrored displays expose one desktop. Sharing is deferred.'
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
