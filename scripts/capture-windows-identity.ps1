# Trusted Windows identity baseline, observed independently of Go and Rust output.
# Capture mode writes a create-only JSON file under RUNNER_TEMP (outside the repo).
# Verify mode re-observes the same runner identity and compares it to that baseline.
# Output contains SIDs only. Messages never include SIDs, user names or paths.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('Capture', 'Verify')][string]$Mode,
    [Parameter(Mandatory = $true)][ValidateSet('X64', 'ARM64')][string]$ExpectedArch,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedRevision,
    [Parameter(Mandatory = $true)][string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'
$schema = 'symeraseme.windows-identity-baseline.v1'

function Fail([string]$message) {
    throw "windows identity baseline: $message"
}

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    Fail 'unsupported platform (not Windows)'
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

    public static string CurrentPrimaryGroupSid()
    {
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, out token)) throw new Win32Exception(Marshal.GetLastWin32Error());
        IntPtr buffer = IntPtr.Zero;
        try
        {
            int needed;
            if (GetTokenInformation(token, TokenPrimaryGroup, IntPtr.Zero, 0, out needed))
                throw new InvalidOperationException("size probe unexpectedly succeeded");
            int probeError = Marshal.GetLastWin32Error();
            if (probeError != ERROR_INSUFFICIENT_BUFFER || needed <= 0) throw new Win32Exception(probeError);
            buffer = Marshal.AllocHGlobal(needed);
            int returned;
            if (!GetTokenInformation(token, TokenPrimaryGroup, buffer, needed, out returned))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            IntPtr sid = Marshal.ReadIntPtr(buffer, 0);
            if (sid == IntPtr.Zero) throw new InvalidOperationException("token has no primary group");
            return SidToString(sid);
        }
        finally
        {
            if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);
            CloseHandle(token);
        }
    }

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

# Source: observed HEAD must equal the dispatched revision and the tree must be clean (capture only).
$repo = Split-Path -Parent $PSScriptRoot
# Windows PowerShell 5.1 turns native stderr into terminating errors under Stop; use Continue for git only.
function GitOut([string[]]$gitArgs) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & git -C $repo @gitArgs; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $previous }
    [pscustomobject]@{ Code = $code; Lines = @($out) }
}
$head = GitOut @('rev-parse', 'HEAD')
Check ($head.Code -eq 0 -and $head.Lines.Count -eq 1 -and $head.Lines[0] -eq $ExpectedRevision) 'source_revision_matches_head'
if ($Mode -eq 'Capture') {
    $dirty = GitOut @('status', '--porcelain', '--untracked-files=all')
    Check ($dirty.Code -eq 0 -and $dirty.Lines.Count -eq 0) 'source_tree_clean'
}

# Native architecture: process must not be WOW64 (processMachine UNKNOWN=0) and native machine must match.
$expectedMachine = @{ 'X64' = [uint16]0x8664; 'ARM64' = [uint16]0xAA64 }[$ExpectedArch]
[uint16]$processMachine = 0
[uint16]$nativeMachine = 0
[SymWinIdentity]::ProcessMachines([ref]$processMachine, [ref]$nativeMachine)
Check ($processMachine -eq 0 -and $nativeMachine -eq $expectedMachine -and [IntPtr]::Size -eq 8) 'native_architecture'

# Identity from the process token and the runner user, not from Rust or Go output.
$userSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
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
    $raw = [System.IO.File]::ReadAllText($ArtifactPath, [System.Text.Encoding]::UTF8)
    if ($raw.Length -gt 16384) { Fail 'baseline artifact too large' }
    $baseline = $raw | ConvertFrom-Json
    Check ($baseline.schema -eq $schema) 'baseline_schema'
    Check ($baseline.source_revision -eq $ExpectedRevision) 'baseline_revision_matches'
    Check ($baseline.process.user_sid -eq $userSid) 'user_sid_matches_baseline'
    Check ($baseline.process.primary_group_sid -eq $primaryGroupSid) 'primary_group_sid_matches_baseline'
    Check ($baseline.local_admin.rid500_sid -eq $rid500[0]) 'rid500_sid_matches_baseline'
    Write-Output "windows identity verify: $($checks.Count) checks passed"
    exit 0
}

# Capture: create-only output under RUNNER_TEMP, outside the repository.
$runnerTemp = $env:RUNNER_TEMP
if ([string]::IsNullOrEmpty($runnerTemp)) { Fail 'RUNNER_TEMP is not set' }
$tempFull = [System.IO.Path]::GetFullPath($runnerTemp).TrimEnd('\')
$repoFull = [System.IO.Path]::GetFullPath($repo).TrimEnd('\')
$outFull = [System.IO.Path]::GetFullPath($ArtifactPath)
Check ($outFull.StartsWith($tempFull + '\', [System.StringComparison]::OrdinalIgnoreCase)) 'artifact_under_runner_temp'
Check (-not $tempFull.StartsWith($repoFull + '\', [System.StringComparison]::OrdinalIgnoreCase)) 'runner_temp_outside_repo'
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
