[CmdletBinding()]
param(
    [int]$Port = 3000,
    [string]$AdminToken = "local-secret",
    [string]$DataDir = (Join-Path $PSScriptRoot "data"),
    [switch]$Release,
    [switch]$NoBuild,
    [switch]$DryRun,
    [switch]$OpenBrowser
)

$ErrorActionPreference = "Stop"
Set-Location -LiteralPath $PSScriptRoot

function Get-ToolPath([string]$Name) {
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        return $null
    }
    return $command.Source
}

$cargo = Get-ToolPath "cargo.exe"
if ($null -eq $cargo) {
    throw "Cargo를 찾을 수 없습니다. Rust toolchain을 설치한 뒤 다시 실행하세요."
}

if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot "Cargo.toml"))) {
    throw "Cargo.toml이 없어 LightPost 프로젝트로 인식할 수 없습니다."
}

$resolvedDataDir = [System.IO.Path]::GetFullPath($DataDir)
New-Item -ItemType Directory -Force -Path $resolvedDataDir | Out-Null

$compiler = Get-ToolPath "clang-cl.exe"
if ($null -eq $compiler) {
    $compiler = Get-ToolPath "cl.exe"
}

$configuration = if ($Release) { "release" } else { "debug" }
$binaryPath = Join-Path $PSScriptRoot (Join-Path "target\$configuration" "lightpost.exe")

Write-Host "LightPost" -ForegroundColor Cyan
Write-Host "  Root:      $PSScriptRoot"
Write-Host "  Address:   http://127.0.0.1:$Port"
Write-Host "  Data:      $resolvedDataDir"
Write-Host "  Build:     $configuration"
Write-Host "  Admin:     LIGHTPOST_ADMIN_TOKEN"

if ($DryRun) {
    Write-Host "Dry run: 서버를 시작하지 않았습니다." -ForegroundColor Yellow
    if ($null -ne $compiler) {
        Write-Host "  C compiler: $compiler"
    } else {
        Write-Host "  C compiler: 미발견 (SQLite bundled 빌드가 실패할 수 있습니다)" -ForegroundColor Yellow
    }
    exit 0
}

$environmentNames = @("LIGHTPOST_PORT", "LIGHTPOST_ADMIN_TOKEN", "LIGHTPOST_DATA_DIR", "CC", "RUST_BACKTRACE")
$previousEnvironment = @{}
foreach ($name in $environmentNames) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

try {
    $env:LIGHTPOST_PORT = $Port.ToString()
    $env:LIGHTPOST_ADMIN_TOKEN = $AdminToken
    $env:LIGHTPOST_DATA_DIR = $resolvedDataDir
    $env:RUST_BACKTRACE = "1"
    if ($null -ne $compiler) {
        $env:CC = [System.IO.Path]::GetFileName($compiler)
    }

    if (-not $NoBuild) {
        $buildArguments = @("build")
        if ($Release) {
            $buildArguments += "--release"
        }
        Write-Host "빌드 중..." -ForegroundColor DarkCyan
        & $cargo @buildArguments
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo 빌드가 실패했습니다. 종료 코드: $LASTEXITCODE"
        }
    }

    if (-not (Test-Path -LiteralPath $binaryPath)) {
        throw "실행 파일을 찾을 수 없습니다: $binaryPath"
    }

    if ($OpenBrowser) {
        Start-Process "http://127.0.0.1:$Port/"
    }

    Write-Host "서버를 시작합니다. 종료하려면 Ctrl+C를 누르세요." -ForegroundColor Green
    & $binaryPath
    if (($LASTEXITCODE -ne 0) -and ($LASTEXITCODE -ne -1) -and ($LASTEXITCODE -ne -1073741510)) {
        throw "LightPost가 종료 코드 $LASTEXITCODE 로 종료되었습니다."
    }
}
finally {
    foreach ($name in $environmentNames) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], "Process")
    }
}
