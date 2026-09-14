param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath
)

$ErrorActionPreference = "Stop"

$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$installDir = [System.IO.Path]::GetFullPath(
    (Join-Path $tempRoot ("llmd-desktop-e2e-" + [guid]::NewGuid().ToString("N")))
)
if (-not $installDir.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use test directory outside the system temp directory: $installDir"
}

$appProcess = $null
try {
    New-Item -ItemType Directory -Path $installDir | Out-Null
    $install = Start-Process -FilePath $installer -ArgumentList @("/S", "/D=$installDir") -Wait -PassThru
    if ($install.ExitCode -ne 0) {
        throw "NSIS installer exited with code $($install.ExitCode)"
    }

    $appPath = Join-Path $installDir "llmd-app.exe"
    $libraryPath = Join-Path $installDir "litert-lm.dll"
    foreach ($path in @($appPath, $libraryPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Packaged file is missing: $path"
        }
    }

    $appProcess = Start-Process -FilePath $appPath -WorkingDirectory $installDir -WindowStyle Hidden -PassThru
    $health = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($appProcess.HasExited) {
            throw "Packaged app exited before becoming healthy with code $($appProcess.ExitCode)"
        }
        try {
            $health = Invoke-RestMethod -Uri "http://127.0.0.1:11435/health" -TimeoutSec 1
            break
        } catch {
            Start-Sleep -Milliseconds 500
        }
    }
    if ($null -eq $health -or $health.status -ne "ok") {
        throw "Packaged app did not become healthy"
    }

    $models = Invoke-RestMethod -Uri "http://127.0.0.1:11435/v1/models" -TimeoutSec 5
    if ($models.object -ne "list" -or $null -eq $models.data) {
        throw "Packaged app returned an invalid OpenAI model list"
    }

    Write-Output "Desktop package E2E test passed."
} finally {
    if ($null -ne $appProcess -and -not $appProcess.HasExited) {
        Stop-Process -Id $appProcess.Id -Force
        Wait-Process -Id $appProcess.Id -ErrorAction SilentlyContinue
    }
    $uninstaller = Join-Path $installDir "uninstall.exe"
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
        $uninstall = Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait -PassThru
        if ($uninstall.ExitCode -ne 0) {
            Write-Warning "NSIS uninstaller exited with code $($uninstall.ExitCode)"
        }
    }
}
