[CmdletBinding()]
param([string]$Exe)
$ErrorActionPreference = 'Stop'
if (-not $Exe) { $Exe = Join-Path $PSScriptRoot '../target/release/mado-screen-poc.exe' }
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class MenuTrial {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    private delegate bool EnumProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll", SetLastError=true)] private static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, UIntPtr w, IntPtr l, uint flags, uint milliseconds, out UIntPtr result);
    public static string Title(IntPtr hwnd) { var text = new StringBuilder(512); GetWindowText(hwnd,text,text.Capacity); return text.ToString(); }
    public static IntPtr Status(int process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr data) { uint p; GetWindowThreadProcessId(h,out p); if(p == process && Title(h).StartsWith("Mado ") && Title(h).Contains("; ")) found=h; return true; },IntPtr.Zero);
        return found;
    }
    public static IntPtr Canvas(int process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr data) { uint p; GetWindowThreadProcessId(h,out p); if(p == process && Title(h) == "Mado Screen PoC 1") found=h; return true; },IntPtr.Zero);
        return found;
    }
    public static void Request(IntPtr owner, uint kind, uint payload) {
        if(!PostMessage(owner,0x8028,new UIntPtr(payload),new IntPtr(kind))) throw new Exception("Owned UI request could not be queued");
    }
    public static void Send(IntPtr hwnd, uint message, uint w, int x, int y) {
        UIntPtr result;
        if(SendMessageTimeout(hwnd,message,new UIntPtr(w),new IntPtr((y << 16) | (x & 65535)),2,2000,out result) == IntPtr.Zero)
            throw new Exception("Owned-window message timed out or failed: " + message);
    }
    public static Rect Bounds(IntPtr hwnd) {
        var prior=SetThreadDpiAwarenessContext(new IntPtr(-4));
        try { Rect r; if(!GetWindowRect(hwnd,out r)) throw new Exception("GetWindowRect failed"); return r; }
        finally { SetThreadDpiAwarenessContext(prior); }
    }
    public static void Capture(Rect r, string path) {
        var prior=SetThreadDpiAwarenessContext(new IntPtr(-4));
        try { using(var b = new System.Drawing.Bitmap(r.Right-r.Left,r.Bottom-r.Top)) using(var g=System.Drawing.Graphics.FromImage(b)) {
            g.CopyFromScreen(r.Left,r.Top,0,0,new System.Drawing.Size(b.Width,b.Height)); b.Save(path,System.Drawing.Imaging.ImageFormat.Png);
        }} finally { SetThreadDpiAwarenessContext(prior); }
    }
}
'@
$existing = [MenuTrial]::FindWindow('MadoScreenPoc1','Mado Screen PoC 1')
if ($existing -ne [IntPtr]::Zero) { throw 'An existing overlay is running. It was not changed; quit it yourself before this targeted trial.' }
$exePath = (Resolve-Path $Exe).Path
$directory = Join-Path $PSScriptRoot '../target/panel-menu-followup/native-trial'
New-Item -ItemType Directory -Force -Path $directory | Out-Null
$before = [MenuTrial]::GetForegroundWindow()
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $exePath
$info.WorkingDirectory = Split-Path $exePath
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$process = [System.Diagnostics.Process]::Start($info)
$checks = New-Object System.Collections.Generic.List[string]
$capture = 'not attempted'
function AssertTrial([bool]$Ok, [string]$Name) {
    if (-not $Ok) { throw "FAIL: $Name" }
    $checks.Add($Name)
    Write-Host "PASS: $Name"
}
function WaitTitle([string]$Expected) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt 2500) {
        if ([MenuTrial]::Title($script:status) -eq $Expected) { return }
        if ($process.HasExited) { throw 'Trial executable exited unexpectedly.' }
        Start-Sleep -Milliseconds 20
    }
    throw "Status did not reach $Expected. Actual: $([MenuTrial]::Title($script:status))"
}
function ExpandTrial {
    $r = [MenuTrial]::Bounds($script:status)
    [MenuTrial]::Send($script:status,0x200,0,($r.Right-$r.Left-18),15)
    $wait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $r=[MenuTrial]::Bounds($script:status)
        if (($r.Right-$r.Left) -ge [Math]::Round(435*$script:scale)) { return $r }
        Start-Sleep -Milliseconds 10
    } while ($wait.ElapsedMilliseconds -lt 1500)
    throw 'Owned menu did not expand in the bounded wait.'
}
try {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $script:status = [MenuTrial]::Status($process.Id)
        if ($script:status -ne [IntPtr]::Zero) { break }
        if ($process.HasExited) { throw 'Trial startup failed, possibly a global shortcut conflict.' }
        Start-Sleep -Milliseconds 20
    } while ($wait.ElapsedMilliseconds -lt 5000)
    AssertTrial ($script:status -ne [IntPtr]::Zero) 'Release app exposes its owned status window'
    WaitTitle 'Mado Pen; Click-through'
    $script:scale = [MenuTrial]::GetDpiForWindow($script:status)/96.0
    $compact = [MenuTrial]::Bounds($script:status)
    AssertTrial (($compact.Right-$compact.Left) -gt [Math]::Round(44*$scale)) 'Compact window includes the brush-tip padding'
    $full = ExpandTrial
    AssertTrial ((($full.Right-$full.Left) -eq [Math]::Round(440*$scale)) -and (($full.Bottom-$full.Top) -eq [Math]::Round(304*$scale))) 'Native expanded dimensions match the logical reference at monitor DPI'
    try { [MenuTrial]::Capture($full,(Join-Path $directory 'desktop-full.png')); $capture='saved, requires visual inspection' }
    catch { $capture="blocked: $($_.Exception.Message)" }
    # These are the same pointer-free queued requests emitted by the menu.
    # Physical cursor delivery is separate; synthetic hover can collapse under a real outside cursor.
    $canvas=[MenuTrial]::Canvas($process.Id)
    AssertTrial ($canvas -ne [IntPtr]::Zero) 'Test canvas belongs to the started process'
    $labels = @('Pen','Temporary pen','Line','Arrow head-first','Rectangle','Ellipse','Eraser')
    foreach ($i in @(1,2,3,4,5,6,0)) {
        [MenuTrial]::Request($canvas,4,[uint32]$i)
        WaitTitle "Mado $($labels[$i]); Click-through"
        AssertTrial $true "Queued menu tool $i reaches committed $($labels[$i]) in click-through"
    }
    [MenuTrial]::Request($canvas,4,3)
    WaitTitle 'Mado Arrow head-first; Click-through'
    [MenuTrial]::Request($canvas,4,3)
    WaitTitle 'Mado Arrow tail-first; Click-through'
    AssertTrial $true 'Repeated selected-Arrow request changes committed direction'
    [MenuTrial]::Request($canvas,6,0)
    WaitTitle 'Mado Arrow tail-first; Drawing'
    AssertTrial (([MenuTrial]::GetWindowLongPtr($canvas,-20).ToInt64() -band 0x20) -eq 0) 'Queued input toggle enables canvas drawing'
    [MenuTrial]::Request($canvas,6,0)
    WaitTitle 'Mado Arrow tail-first; Click-through'
    AssertTrial (([MenuTrial]::GetWindowLongPtr($canvas,-20).ToInt64() -band 0x20) -ne 0) 'Queued input toggle returns canvas to click-through'
    [MenuTrial]::Send($status,0x2A3,0,0,0)
    AssertTrial ([MenuTrial]::GetForegroundWindow() -eq $before) 'Trial startup and posted menu actions did not change foreground focus'
} finally {
    if (-not $process.HasExited) {
        $canvas=[MenuTrial]::Canvas($process.Id)
        if($canvas -ne [IntPtr]::Zero) { [MenuTrial]::Send($canvas,0x10,0,0,0) }
        if(-not $process.WaitForExit(5000)) { $process.Kill(); $process.WaitForExit() }
    }
    [pscustomobject]@{Executable=$exePath;Checks=$checks.ToArray();DesktopCapture=$capture;DpiScale=$script:scale;InputScope='Owned windows and queued UI requests; physical mouse/keyboard delivery not tested';SystemInputInjected=$false;ExitCode=$process.ExitCode} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $directory 'results.json')
    $process.Dispose()
}
