# Installa piadazip — https://github.com/Tnnienn/piadazip
# Uso: irm https://raw.githubusercontent.com/Tnnienn/piadazip/main/install.ps1 | iex
#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Repo    = "Tnnienn/piadazip"
$Bin     = "piadazip.exe"
$Archive = "piadazip-windows-x86_64.zip"

# ── Ultima versione ───────────────────────────────────────────────────────────

$Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$Version = $Release.tag_name

# ── Download ──────────────────────────────────────────────────────────────────

$Url = "https://github.com/$Repo/releases/download/$Version/$Archive"
Write-Host "Installazione piadazip $Version (windows-x86_64)..."

$Tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "piadazip-install-$(Get-Random)")
try {
    $ZipPath = Join-Path $Tmp $Archive
    Invoke-WebRequest $Url -OutFile $ZipPath -UseBasicParsing
    Expand-Archive $ZipPath -DestinationPath $Tmp -Force

    # ── Installazione ─────────────────────────────────────────────────────────

    $InstallDir = Join-Path $env:LOCALAPPDATA "Programs\piadazip"
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item (Join-Path $Tmp $Bin) (Join-Path $InstallDir $Bin) -Force

    # Aggiungi al PATH utente se non presente
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        Write-Host "Aggiunto $InstallDir al PATH utente."
        Write-Host "Riavvia il terminale per attivare il comando 'piadazip'."
    }

    Write-Host ""
    Write-Host "piadazip $Version installato in $InstallDir\$Bin"
    Write-Host "Prova: piadazip --help"
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
