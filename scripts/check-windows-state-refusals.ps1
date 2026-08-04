$ErrorActionPreference = "Stop"

# Native Windows acceptance for X-127. The primitive ACL tests live beside the Win32 binding; this
# script crosses the real binary/process boundary and proves the complete startup composition
# refuses the same planted metadata before parsing a store value.
if (-not $IsWindows) {
    throw "check-windows-state-refusals.ps1 must run on native Windows"
}

$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    cargo build --locked --bin flux-exchange
    if ($LASTEXITCODE -ne 0) { throw "could not build flux-exchange" }

    $binary = Join-Path $repo "target/debug/flux-exchange.exe"
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("flux-exchange-x127-windows-" + [guid]::NewGuid().ToString("N"))
    $stateRoot = Join-Path $scratch "state"
    $stdout = Join-Path $scratch "server.stdout"
    $stderr = Join-Path $scratch "server.stderr"
    $sentinel = "X127-SENTINEL-NOT-A-REAL-SECRET-WINDOWS-ACL"
    $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $everyoneSid = [System.Security.Principal.SecurityIdentifier]::new("S-1-1-0")
    $administratorsSid = [System.Security.Principal.SecurityIdentifier]::new("S-1-5-32-544")

    New-Item -ItemType Directory -Path $scratch | Out-Null

    function Start-Exchange {
        Remove-Item -Force -ErrorAction SilentlyContinue $stdout, $stderr
        $priorState = $env:FLUX_EXCHANGE_STATE
        $priorBind = $env:FLUX_EXCHANGE_BIND
        $priorUser = $env:USER
        $priorNoColor = $env:NO_COLOR
        $priorCargoColor = $env:CARGO_TERM_COLOR
        try {
            $env:FLUX_EXCHANGE_STATE = $stateRoot
            $env:FLUX_EXCHANGE_BIND = "127.0.0.1:0"
            $env:USER = "windows-acl-fixture"
            $env:NO_COLOR = "1"
            $env:CARGO_TERM_COLOR = "never"
            $process = Start-Process -FilePath $binary -ArgumentList "--dev" -PassThru `
                -RedirectStandardOutput $stdout -RedirectStandardError $stderr
            return $process
        }
        finally {
            $env:FLUX_EXCHANGE_STATE = $priorState
            $env:FLUX_EXCHANGE_BIND = $priorBind
            $env:USER = $priorUser
            $env:NO_COLOR = $priorNoColor
            $env:CARGO_TERM_COLOR = $priorCargoColor
        }
    }

    function Wait-UntilListening([System.Diagnostics.Process] $process) {
        for ($attempt = 0; $attempt -lt 400; $attempt++) {
            if ($process.HasExited) {
                $output = (Get-Content -Raw -ErrorAction SilentlyContinue $stdout) + (Get-Content -Raw -ErrorAction SilentlyContinue $stderr)
                throw "fixture server exited before listening: $output"
            }
            $output = (Get-Content -Raw -ErrorAction SilentlyContinue $stdout) + (Get-Content -Raw -ErrorAction SilentlyContinue $stderr)
            if ($output -match "local=127\.0\.0\.1:[0-9]+") { return }
            Start-Sleep -Milliseconds 50
        }
        throw "fixture server did not listen"
    }

    function Stop-Exchange([System.Diagnostics.Process] $process) {
        if (-not $process.HasExited) { $process.Kill() }
        $process.WaitForExit()
    }

    function New-OwnerRule([bool] $inheritable) {
        if ($inheritable) {
            return [System.Security.AccessControl.FileSystemAccessRule]::new(
                $currentSid,
                [System.Security.AccessControl.FileSystemRights]::FullControl,
                [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
                [System.Security.AccessControl.PropagationFlags]::None,
                [System.Security.AccessControl.AccessControlType]::Allow
            )
        }
        return [System.Security.AccessControl.FileSystemAccessRule]::new(
            $currentSid,
            [System.Security.AccessControl.FileSystemRights]::FullControl,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    }

    function Set-PrivateFile([string] $path) {
        [System.IO.File]::WriteAllText($path, $sentinel)
        $acl = [System.Security.AccessControl.FileSecurity]::new()
        $acl.SetOwner($currentSid)
        $acl.SetAccessRuleProtection($true, $false)
        [void]$acl.AddAccessRule((New-OwnerRule $false))
        Set-Acl -LiteralPath $path -AclObject $acl
    }

    function Assert-StartupRefusal([string] $label, [string] $path, [string] $beforeSddl) {
        $process = Start-Exchange
        if (-not $process.WaitForExit(30000)) {
            Stop-Exchange $process
            throw "$label metadata was admitted and the server kept running"
        }
        $output = (Get-Content -Raw -ErrorAction SilentlyContinue $stdout) + (Get-Content -Raw -ErrorAction SilentlyContinue $stderr)
        if ($process.ExitCode -eq 0) { throw "$label metadata did not refuse startup" }
        if (-not $output.Contains($path)) { throw "$label refusal did not name $path`: $output" }
        if ($output.Contains($sentinel)) { throw "$label refusal disclosed the planted value" }
        $afterSddl = (Get-Acl -LiteralPath $path).Sddl
        if ($afterSddl -ne $beforeSddl) { throw "$label refusal repaired the planted metadata" }
        if ([System.IO.File]::ReadAllText($path) -ne $sentinel) { throw "$label refusal changed store bytes" }
    }

    # Let the real composition create the root and every store directory with the production
    # process-SID/protected-DACL binding. The grant file is then planted beneath that exact layout.
    $healthy = Start-Exchange
    Wait-UntilListening $healthy
    Stop-Exchange $healthy

    $grantDirectory = Join-Path $stateRoot "grants"
    $grantPath = Join-Path $grantDirectory "store.json"

    Set-PrivateFile $grantPath
    $broadAcl = Get-Acl -LiteralPath $grantPath
    [void]$broadAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
        $everyoneSid,
        [System.Security.AccessControl.FileSystemRights]::Read,
        [System.Security.AccessControl.AccessControlType]::Allow
    ))
    Set-Acl -LiteralPath $grantPath -AclObject $broadAcl
    Assert-StartupRefusal "broad DACL" $grantPath (Get-Acl -LiteralPath $grantPath).Sddl

    Set-PrivateFile $grantPath
    $foreignOwnerAcl = Get-Acl -LiteralPath $grantPath
    $foreignOwnerAcl.SetOwner($administratorsSid)
    Set-Acl -LiteralPath $grantPath -AclObject $foreignOwnerAcl
    Assert-StartupRefusal "foreign owner" $grantPath (Get-Acl -LiteralPath $grantPath).Sddl

    # An explicit inheritable owner rule on the parent produces a genuinely inherited ACE on the
    # newly created file. The parent remains protected and owner-only, so startup reaches the child
    # check and refuses specifically because the child descriptor inherited authority.
    $parentAcl = [System.Security.AccessControl.DirectorySecurity]::new()
    $parentAcl.SetOwner($currentSid)
    $parentAcl.SetAccessRuleProtection($true, $false)
    [void]$parentAcl.AddAccessRule((New-OwnerRule $true))
    Set-Acl -LiteralPath $grantDirectory -AclObject $parentAcl
    Remove-Item -Force $grantPath
    [System.IO.File]::WriteAllText($grantPath, $sentinel)
    $inheritedSddl = (Get-Acl -LiteralPath $grantPath).Sddl
    if ($inheritedSddl -notmatch "ID") { throw "fixture did not plant an inherited ACE: $inheritedSddl" }
    Assert-StartupRefusal "inherited ACE" $grantPath $inheritedSddl
}
finally {
    if ($scratch -and (Test-Path $scratch)) { Remove-Item -Recurse -Force $scratch }
    Pop-Location
}
