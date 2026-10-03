param([Parameter(Mandatory=$true)][string]$Path, [string]$Mode = "read")
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
if ($Mode -eq "protect") {
    # Caller supplies only its newly created, owned disposable directory.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $acl = Get-Acl -LiteralPath $Path
    $acl.SetAccessRuleProtection($true, $false)
    $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $sid, "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow")
    $acl.SetAccessRule($rule)
    Set-Acl -LiteralPath $Path -AclObject $acl
}
$acl = Get-Acl -LiteralPath $Path
@{
    sddl = $acl.Sddl
    protected = $acl.AreAccessRulesProtected
    readonly = [bool](([System.IO.File]::GetAttributes($Path) -band [System.IO.FileAttributes]::ReadOnly) -ne 0)
} | ConvertTo-Json -Compress
