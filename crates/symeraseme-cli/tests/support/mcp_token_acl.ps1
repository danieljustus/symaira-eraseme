param([Parameter(Mandatory=$true)][string]$Path, [string]$Mode = "read")
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::Error.WriteLine("MCP token ACL script started: " + $Mode)
$directory = [System.IO.Directory]::Exists($Path)
# Use the framework APIs directly: cmdlet module auto-discovery in a cleared
# private environment is not part of the filesystem/DACL contract.
$acl = if ($directory) { [System.IO.Directory]::GetAccessControl($Path) } else { [System.IO.File]::GetAccessControl($Path) }
if ($Mode -eq "protect") {
    # Caller supplies only its newly created, owned disposable directory.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $acl.SetAccessRuleProtection($true, $false)
    $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $sid, "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow")
    $acl.SetAccessRule($rule)
    [System.IO.Directory]::SetAccessControl($Path, $acl)
}
$acl = if ($directory) { [System.IO.Directory]::GetAccessControl($Path) } else { [System.IO.File]::GetAccessControl($Path) }
$sddl = $acl.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::All)
$protected = $acl.AreAccessRulesProtected.ToString().ToLowerInvariant()
$readonly = ([bool](([System.IO.File]::GetAttributes($Path) -band [System.IO.FileAttributes]::ReadOnly) -ne 0)).ToString().ToLowerInvariant()
# SDDL contains identifiers and ACL syntax, never quoted JSON content.
if ($sddl.Contains('"') -or $sddl.Contains('\')) { throw "unexpected SDDL JSON character" }
[Console]::WriteLine('{"sddl":"' + $sddl + '","protected":' + $protected + ',"readonly":' + $readonly + '}')
[Console]::Error.WriteLine("MCP token ACL script completed: " + $Mode)
