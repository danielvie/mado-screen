# PreToolUse guard for agent shell commands.
# Blocks Windows PowerShell (powershell.exe) and nested pwsh calls that pass a
# script block or -EncodedCommand. PowerShell turns a script block passed to a
# child PowerShell into -EncodedCommand, which EDR (Defender) flags as obfuscation.
# Exit 2 blocks the tool call; stderr is shown to the agent.
$raw = [Console]::In.ReadToEnd()
$script:commands = @()
function Find-Command($node) {
    if ($node -is [array]) { foreach ($item in $node) { Find-Command $item } }
    elseif ($node -is [System.Management.Automation.PSCustomObject]) {
        foreach ($p in $node.PSObject.Properties) {
            if ($p.Name -eq 'command' -and $p.Value -is [string]) { $script:commands += $p.Value }
            else { Find-Command $p.Value }
        }
    }
}
try { Find-Command ($raw | ConvertFrom-Json) } catch { }
# ponytail: payload shape is not documented; if no "command" field is found, scan the raw text.
if (-not $script:commands) { $script:commands = @($raw) }

$fix = 'Run it directly in the current pwsh session, or write a .ps1 file and use: pwsh -NoProfile -File <script.ps1>'
$rules = @(
    @{ Pattern = '(?i)(^|[\s;&|(''"\\/])powershell(\.exe)?(?=$|[\s''";|)])'; Reason = "Windows PowerShell (powershell.exe) is not allowed. $fix" }
    @{ Pattern = '(?i)\bpwsh(\.exe)?\b[^;|\r\n]*\{'; Reason = "Passing a script block to pwsh is not allowed (PowerShell encodes it as -EncodedCommand). $fix" }
    @{ Pattern = '(?i)\bpwsh(\.exe)?\b[^;|\r\n]*\s-(e|ec|enc|encodedcommand)\b'; Reason = "-EncodedCommand is not allowed. $fix" }
)
foreach ($command in $script:commands) {
    foreach ($rule in $rules) {
        if ($command -match $rule.Pattern) {
            [Console]::Error.WriteLine("Blocked by hook block-windows-powershell: $($rule.Reason)")
            exit 2
        }
    }
}
exit 0
