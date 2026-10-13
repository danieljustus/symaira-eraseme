# Trusted Windows identity baseline, observed independently of Go and Rust output.
# Capture mode writes a create-only JSON file under RUNNER_TEMP (outside the repo).
# Verify mode re-observes the same runner identity and compares it to that baseline.
# Output contains SIDs only. Messages never include SIDs, user names or paths.
#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('Capture', 'Verify')][string]$Mode,
    [Parameter(Mandatory = $true)][ValidateSet('X64', 'ARM64')][string]$ExpectedArch,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedRevision,
    [Parameter(Mandatory = $true)][string]$ArtifactPath,
    [string]$BashScript = ''
)

$ErrorActionPreference = 'Stop'
$schema = 'symeraseme.windows-identity-baseline.v1'

function Fail([string]$message) {
    throw "windows identity baseline: $message"
}

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    Fail 'unsupported platform (not Windows)'
}
if ($Mode -eq 'Capture' -and -not [string]::IsNullOrEmpty($BashScript)) {
    Fail 'a capture command requires Verify mode'
}

# Official APIs: GetTokenInformation (TokenPrimaryGroup = 5, TOKEN_PRIMARY_GROUP holds one PSID pointer),
# ConvertSidToStringSidW/ConvertStringSidToSidW (LocalFree on returned buffers), IsWow64Process2.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class SymWinIdentity
{
    const uint TOKEN_QUERY = 0x0008;
    const int TokenUser = 1;
    const int TokenPrimaryGroup = 5;
    const int ERROR_INSUFFICIENT_BUFFER = 122;

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr info, int length, out int returnLength);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool ConvertSidToStringSidW(IntPtr sid, out IntPtr sidString);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool ConvertStringSidToSidW(string sidString, out IntPtr sid);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr LocalFree(IntPtr mem);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool IsWow64Process2(IntPtr process, out ushort processMachine, out ushort nativeMachine);

    static string SidToString(IntPtr sid)
    {
        IntPtr text;
        if (!ConvertSidToStringSidW(sid, out text)) throw new Win32Exception(Marshal.GetLastWin32Error());
        try { return Marshal.PtrToStringUni(text); }
        finally { LocalFree(text); }
    }

    // TOKEN_USER and TOKEN_PRIMARY_GROUP both begin with their PSID pointer.
    static string TokenSid(IntPtr process, int infoClass)
    {
        IntPtr token;
        if (!OpenProcessToken(process, TOKEN_QUERY, out token)) throw new Win32Exception(Marshal.GetLastWin32Error());
        IntPtr buffer = IntPtr.Zero;
        try
        {
            int needed;
            if (GetTokenInformation(token, infoClass, IntPtr.Zero, 0, out needed))
                throw new InvalidOperationException("size probe unexpectedly succeeded");
            int probeError = Marshal.GetLastWin32Error();
            if (probeError != ERROR_INSUFFICIENT_BUFFER || needed <= 0) throw new Win32Exception(probeError);
            buffer = Marshal.AllocHGlobal(needed);
            int returned;
            if (!GetTokenInformation(token, infoClass, buffer, needed, out returned))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            IntPtr sid = Marshal.ReadIntPtr(buffer, 0);
            if (sid == IntPtr.Zero) throw new InvalidOperationException("token has no requested SID");
            return SidToString(sid);
        }
        finally
        {
            if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);
            CloseHandle(token);
        }
    }

    public static string CurrentUserSid() { return TokenSid(GetCurrentProcess(), TokenUser); }
    public static string CurrentPrimaryGroupSid() { return TokenSid(GetCurrentProcess(), TokenPrimaryGroup); }
    public static string ProcessUserSid(IntPtr process) { return TokenSid(process, TokenUser); }
    public static string ProcessPrimaryGroupSid(IntPtr process) { return TokenSid(process, TokenPrimaryGroup); }

    public static string AliasSid(string alias)
    {
        IntPtr sid;
        if (!ConvertStringSidToSidW(alias, out sid)) throw new Win32Exception(Marshal.GetLastWin32Error());
        try { return SidToString(sid); }
        finally { LocalFree(sid); }
    }

    public static void ProcessMachines(out ushort processMachine, out ushort nativeMachine)
    {
        if (!IsWow64Process2(GetCurrentProcess(), out processMachine, out nativeMachine))
            throw new Win32Exception(Marshal.GetLastWin32Error());
    }
}
'@

$checks = [ordered]@{}
function Check([bool]$ok, [string]$name) {
    if (-not $ok) { Fail "check failed: $name" }
    $checks[$name] = $true
}

# Source: both capture and launch must use the clean dispatched revision.
$repo = Split-Path -Parent $PSScriptRoot
# Collect git's exit code explicitly without treating native stderr as a PowerShell error.
function GitOut([string[]]$gitArgs) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & git -C $repo @gitArgs; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $previous }
    [pscustomobject]@{ Code = $code; Lines = @($out) }
}
$head = GitOut @('rev-parse', 'HEAD')
Check ($head.Code -eq 0 -and $head.Lines.Count -eq 1 -and $head.Lines[0] -eq $ExpectedRevision) 'source_revision_matches_head'
$dirty = GitOut @('status', '--porcelain', '--untracked-files=all')
Check ($dirty.Code -eq 0 -and $dirty.Lines.Count -eq 0) 'source_tree_clean'

# Native architecture: process must not be WOW64 (processMachine UNKNOWN=0) and native machine must match.
$expectedMachine = @{ 'X64' = [uint16]0x8664; 'ARM64' = [uint16]0xAA64 }[$ExpectedArch]
[uint16]$processMachine = 0
[uint16]$nativeMachine = 0
[SymWinIdentity]::ProcessMachines([ref]$processMachine, [ref]$nativeMachine)
Check ($processMachine -eq 0 -and $nativeMachine -eq $expectedMachine -and [IntPtr]::Size -eq 8) 'native_architecture'

# Identity from the process token and the runner user, not from Rust or Go output.
$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
try { $userSid = $identity.User.Value } finally { $identity.Dispose() }
Check ($userSid -eq [SymWinIdentity]::CurrentUserSid()) 'user_matches_launch_process_token'
$primaryGroupSid = [SymWinIdentity]::CurrentPrimaryGroupSid()
Check ($userSid -match '^S-1-5-[0-9-]+$') 'user_sid_observed'
Check ($primaryGroupSid -match '^S-1-5-[0-9-]+$') 'primary_group_sid_observed'

# Local RID 500 account: observed through WMI Win32_UserAccount (SAM-backed), must be exactly one.
# Compared to the SDDL alias LA converted by the OS, not a string synthesized from the machine name.
$rid500 = @(Get-CimInstance -ClassName Win32_UserAccount -Filter 'LocalAccount=True' |
    Where-Object { $_.SID -match '-500$' } | ForEach-Object { $_.SID })
Check ($rid500.Count -eq 1 -and $rid500[0] -match '^S-1-5-[0-9-]+-500$') 'rid500_local_account_unique'
$laSid = [SymWinIdentity]::AliasSid('LA')
Check ($laSid -eq $rid500[0]) 'rid500_matches_sddl_la'

if ($Mode -eq 'Verify') {
    if (-not (Test-Path -LiteralPath $ArtifactPath -PathType Leaf)) { Fail 'baseline artifact missing' }
    if ((Get-Item -LiteralPath $ArtifactPath).Length -gt 16384) { Fail 'baseline artifact too large' }
    $raw = [System.IO.File]::ReadAllText($ArtifactPath, [System.Text.Encoding]::UTF8)
    if ($raw.Length -gt 16384) { Fail 'baseline artifact too large' }
    $baseline = $raw | ConvertFrom-Json
    Check ($baseline.schema -eq $schema) 'baseline_schema'
    Check ($baseline.source_revision -eq $ExpectedRevision) 'baseline_revision_matches'
    Check ($baseline.script_sha256 -eq (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()) 'baseline_script_matches'
    Check ($baseline.host.platform -eq 'Windows' -and $baseline.host.expected_arch -eq $ExpectedArch -and $baseline.host.native_machine -eq ('0x{0:X4}' -f $nativeMachine) -and $baseline.host.process_machine -eq '0x0000' -and $baseline.host.pointer_size -eq [IntPtr]::Size) 'baseline_native_host_matches'
    Check ($baseline.process.user_sid -eq $userSid) 'user_sid_matches_baseline'
    Check ($baseline.process.primary_group_sid -eq $primaryGroupSid) 'primary_group_sid_matches_baseline'
    Check ($baseline.local_admin.rid500_sid -eq $rid500[0]) 'rid500_sid_matches_baseline'
    Check ($baseline.local_admin.sddl_la_sid -eq $laSid) 'sddl_la_matches_baseline'
    Write-Output "windows identity verify: $($checks.Count) checks passed"
    if (-not [string]::IsNullOrEmpty($BashScript)) {
        # The workflows use pwsh 7. Never resolve WSL bash via PATH or quote
        # a script body into native argv. Hold stdin until the child token matches.
        $bash = Join-Path $env:ProgramFiles 'Git/usr/bin/bash.exe'
        Check (Test-Path -LiteralPath $bash -PathType Leaf) 'git_bash_exists'
        $start = New-Object System.Diagnostics.ProcessStartInfo
        $start.FileName = $bash
        $start.WorkingDirectory = $repo
        $start.Arguments = '-e -o pipefail -s'
        $start.UseShellExecute = $false
        $start.RedirectStandardInput = $true
        $start.StandardInputEncoding = New-Object System.Text.UTF8Encoding($false)
        $child = [System.Diagnostics.Process]::Start($start)
        try {
            Check ([SymWinIdentity]::ProcessUserSid($child.Handle) -eq $userSid) 'child_user_matches_baseline'
            Check ([SymWinIdentity]::ProcessPrimaryGroupSid($child.Handle) -eq $primaryGroupSid) 'child_primary_group_matches_baseline'
            Write-Output 'windows identity child token: user and primary group matched'
            $probe = @'
export PATH="/usr/bin:$PATH"
case "$(uname -s)" in MINGW*|MSYS*) ;; *) exit 91 ;; esac
test -z "$(cat)"
quote='space "quoted" value'
test "$(printf '%s' "$quote")" = 'space "quoted" value'
printf '%s\n' 'identity-launch-probe: Git Bash stdin quotes ok'
'@
            # Bash parses the complete group before executing it. Give every
            # command empty stdin so no child can consume the remaining script.
            $payload = "{`n" + $probe + "`n" + $BashScript + "`n} </dev/null`n"
            $child.StandardInput.Write($payload.Replace("`r`n", "`n"))
            $child.StandardInput.Close()
            if (-not $child.WaitForExit([int]([TimeSpan]::FromMinutes(24).TotalMilliseconds))) {
                Fail 'capture child exceeded its deadline'
            }
            $code = $child.ExitCode
        } finally {
            try { if (-not $child.HasExited) { $child.Kill($true); $child.WaitForExit() } }
            finally { $child.Dispose() }
        }
        exit $code
    }
    exit 0
}

# Capture: create-only output under RUNNER_TEMP, outside the repository.
$runnerTemp = $env:RUNNER_TEMP
if ([string]::IsNullOrEmpty($runnerTemp)) { Fail 'RUNNER_TEMP is not set' }
$tempFull = [System.IO.Path]::GetFullPath($runnerTemp).TrimEnd('\')
$repoFull = [System.IO.Path]::GetFullPath($repo).TrimEnd('\')
$outFull = [System.IO.Path]::GetFullPath($ArtifactPath)
Check ($outFull.StartsWith($tempFull + '\', [System.StringComparison]::OrdinalIgnoreCase)) 'artifact_under_runner_temp'
Check ($tempFull -ne $repoFull -and -not $tempFull.StartsWith($repoFull + '\', [System.StringComparison]::OrdinalIgnoreCase)) 'runner_temp_outside_repo'
Check (-not (Test-Path -LiteralPath $outFull)) 'output_does_not_exist'

$record = [ordered]@{
    schema          = $schema
    observed_at_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    source_revision = $ExpectedRevision
    script_sha256   = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
    host            = [ordered]@{
        platform        = 'Windows'
        expected_arch   = $ExpectedArch
        process_machine = ('0x{0:X4}' -f $processMachine)
        native_machine  = ('0x{0:X4}' -f $nativeMachine)
        pointer_size    = [IntPtr]::Size
    }
    process         = [ordered]@{
        identity_source   = 'WindowsIdentity.GetCurrent plus GetTokenInformation(TokenPrimaryGroup)'
        user_sid          = $userSid
        primary_group_sid = $primaryGroupSid
    }
    local_admin     = [ordered]@{
        observed_via   = 'Win32_UserAccount LocalAccount=True SID suffix -500'
        alias_observed = 'ConvertStringSidToSidW(LA)'
        rid500_sid     = $rid500[0]
        sddl_la_sid    = $laSid
    }
}

$record['checks'] = $checks
$json = $record | ConvertTo-Json -Depth 6
$bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($json + "`n")
if ($bytes.Length -gt 16384) { Fail 'output too large' }
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($outFull)) | Out-Null
try {
    $stream = [System.IO.File]::Open($outFull, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
} catch {
    Fail 'create-only output write failed'
}
Write-Output "windows identity baseline: captured $($checks.Count) checks, file $([System.IO.Path]::GetFileName($outFull))"
