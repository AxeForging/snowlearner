# Installs snowlearner in one step (Windows x86_64):
#   irm https://raw.githubusercontent.com/AxeForging/snowlearner/main/install.ps1 | iex
# Downloads the latest release, checks its SHA-256, installs to
# %LOCALAPPDATA%\Programs\snowlearner, adds it to your PATH and the Start menu,
# then runs `snowlearner setup`. $env:SNOWLEARNER_LANG = "es" picks Spanish.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'AxeForging/snowlearner'
$target = 'x86_64-pc-windows-msvc'
$asset = "snowlearner-$target.zip"
$version = if ($env:SNOWLEARNER_VERSION) { $env:SNOWLEARNER_VERSION } else { 'latest' }
$base = if ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }
$dir = Join-Path $env:LOCALAPPDATA 'Programs\snowlearner'
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("snowlearner-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp, $dir | Out-Null

try {
    Write-Host "Baixando snowlearner ($target)..."
    Invoke-WebRequest "$base/$asset" -OutFile "$tmp\$asset"
    Invoke-WebRequest "$base/$asset.sha256" -OutFile "$tmp\$asset.sha256"
    $expected = ((Get-Content "$tmp\$asset.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash "$tmp\$asset" -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) { throw "checksum não confere (esperado $expected, veio $actual)" }

    Expand-Archive "$tmp\$asset" -DestinationPath $tmp -Force
    Copy-Item "$tmp\snowlearner-$target\snowlearner.exe" "$dir\snowlearner.exe" -Force
    Write-Host "Instalado em $dir\snowlearner.exe"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' | Where-Object { $_ -eq $dir })) {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User')
        $env:Path = "$env:Path;$dir"
        Write-Host "Adicionado ao PATH (abra um novo terminal para usar 'snowlearner')."
    }

    $startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Snowlearner.lnk'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($startMenu)
    $link.TargetPath = "$dir\snowlearner.exe"
    $link.Arguments = 'run'
    $link.Save()
    Write-Host "Atalho criado no menu Iniciar."
    Write-Host ""

    $setupArgs = @('setup')
    if ($env:SNOWLEARNER_LANG) { $setupArgs += @('--lang', $env:SNOWLEARNER_LANG) }
    if ($env:SNOWLEARNER_NATIVE) { $setupArgs += @('--native', $env:SNOWLEARNER_NATIVE) }
    & "$dir\snowlearner.exe" @setupArgs
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
