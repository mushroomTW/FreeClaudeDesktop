# 從 GitHub Release 安裝預編譯 binary；不需要 Rust、Cargo 或 Git。
[CmdletBinding()]
param(
    [string]$ReleaseBaseUrl = 'https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'FreeClaudeDesktop\bin')
)

$ErrorActionPreference = 'Stop'

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
$target = switch ($architecture) {
    'X64' { 'x86_64-pc-windows-msvc' }
    'Arm64' { 'aarch64-pc-windows-msvc' }
    default { throw "不支援的 Windows CPU 架構：$architecture" }
}

$archiveName = "freeclaude-$target.zip"
$workDir = Join-Path ([System.IO.Path]::GetTempPath()) ("freeclaude-" + [guid]::NewGuid())
try {
    New-Item -ItemType Directory -Force -Path $workDir | Out-Null
    $archivePath = Join-Path $workDir $archiveName
    Invoke-WebRequest -Uri "$ReleaseBaseUrl/$archiveName" -OutFile $archivePath
    $checksumsPath = Join-Path $workDir 'checksums.txt'
    Invoke-WebRequest -Uri "$ReleaseBaseUrl/checksums.txt" -OutFile $checksumsPath
    $escapedArchiveName = [regex]::Escape($archiveName)
    $checksumMatch = Select-String -Path $checksumsPath -Pattern "^\s*([a-fA-F0-9]{64})\s+\*?$escapedArchiveName\s*$" | Select-Object -First 1
    if (-not $checksumMatch) {
        throw "checksums.txt 不包含 $archiveName。"
    }
    $expectedChecksum = $checksumMatch.Matches[0].Groups[1].Value.ToLowerInvariant()
    $actualChecksum = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expectedChecksum -ne $actualChecksum) {
        throw '下載檔案的 SHA-256 驗證失敗。'
    }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $workDir -Force

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item (Join-Path $workDir 'freeclaude.exe') (Join-Path $InstallDir 'freeclaude.exe') -Force
    Copy-Item (Join-Path $workDir 'freeclaude-proxy.exe') (Join-Path $InstallDir 'freeclaude-proxy.exe') -Force

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';') + ';' + $InstallDir).TrimStart(';')), 'User')
    }
    $env:Path = "$InstallDir;$env:Path"

    Write-Host "FreeClaudeDesktop 已安裝至：$InstallDir"
    Write-Host '下一步：freeclaude install'
}
finally {
    if (Test-Path $workDir) {
        Remove-Item -LiteralPath $workDir -Recurse -Force
    }
}
