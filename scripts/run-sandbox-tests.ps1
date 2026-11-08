$ErrorActionPreference = 'Continue'

$EvidenceDir = Join-Path $PSScriptRoot 'evidence'
if (-not (Test-Path -Path $EvidenceDir)) {
    New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null
}

if ($env:ESTZKIT_CLI) {
    $Cli = $env:ESTZKIT_CLI
} elseif (Test-Path -Path '.\estzkit-cli.exe') {
    $Cli = (Resolve-Path '.\estzkit-cli.exe').Path
} else {
    $found = Get-ChildItem -Path 'C:\EstzKit' -Filter 'estzkit-cli.exe' -Recurse -File -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) {
        $Cli = $found.FullName
    } else {
        Write-Host 'ERROR: estzkit-cli.exe not found. Download the estzkit-cli CI artifact (target/release/estzkit-cli.exe) from the latest workflow run and set $env:ESTZKIT_CLI or place estzkit-cli.exe in the current directory.' -ForegroundColor Red
        exit 1
    }
}

Write-Host "Using CLI: $Cli"

# Scenario 1: clean install proves exit 0 via EstzKit CLI.
& $Cli install git *> (Join-Path $EvidenceDir 'scenario1-install.log')
"$LASTEXITCODE" | Out-File -FilePath (Join-Path $EvidenceDir 'scenario1-exit.txt') -Encoding utf8
& $Cli detect *> (Join-Path $EvidenceDir 'scenario1-detect.log')

# Scenario 2: missing package proves the nonzero path via EstzKit CLI and winget.
# Create a test fixture manifest that resolves in EstzKit but maps to a nonexistent winget ID.
$TestManifestDir = Join-Path $EvidenceDir 'test-manifests'
$TestPkgDir = Join-Path $TestManifestDir 'packages'
if (-not (Test-Path -Path $TestPkgDir)) {
    New-Item -ItemType Directory -Path $TestPkgDir -Force | Out-Null
}
@"
schema_version: 1
name: probe-missing
category: tool
windows:
  provider: winget
  package: EstzKit.Nonexistent.Package.Probe
install:
  strategy: package_manager
  scope: user
verify:
  command: probe-missing
  args: ["--version"]
security:
  allow_scripts: false
  allow_elevation: false
"@ | Out-File -FilePath (Join-Path $TestPkgDir 'probe-missing.yaml') -Encoding utf8

& $Cli --manifest-dir $TestManifestDir install probe-missing *> (Join-Path $EvidenceDir 'scenario2-install.log')
"$LASTEXITCODE" | Out-File -FilePath (Join-Path $EvidenceDir 'scenario2-exit.txt') -Encoding utf8
& $Cli --manifest-dir $TestManifestDir detect *> (Join-Path $EvidenceDir 'scenario2-detect.log')

Write-Host "Evidence written to $EvidenceDir :"
Write-Host '  - evidence/scenario1-install.log'
Write-Host '  - evidence/scenario1-exit.txt'
Write-Host '  - evidence/scenario1-detect.log'
Write-Host '  - evidence/scenario2-install.log'
Write-Host '  - evidence/scenario2-exit.txt'
Write-Host '  - evidence/scenario2-detect.log'
Write-Host '  - estzkit.db (SQLite recording of status)'
Write-Host 'Paste the values into the playbook Observed column in docs/vm-testing-playbook.md.'
