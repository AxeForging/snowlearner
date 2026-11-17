# Installs snowlearner in one step (Windows x86_64):
#   irm https://raw.githubusercontent.com/AxeForging/snowlearner/main/install.ps1 | iex
# Downloads the latest release, checks its SHA-256, closes the app if it is
# open, installs to %LOCALAPPDATA%\Programs\snowlearner, adds it to your PATH
# and the Start menu, then runs `snowlearner setup`. $env:SNOWLEARNER_LANG = "es" picks Spanish.
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

    # Windows does not let a running exe be overwritten: close the open app first.
    $exe = "$dir\snowlearner.exe"
    $open = @(Get-Process snowlearner -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe })
    if ($open.Count -gt 0) {
        Write-Host "Fechando o Snowlearner aberto para atualizar..."
        # `quit` may print to stderr (app not answering); with 'Stop' PowerShell 5.1 would abort on that.
        $ErrorActionPreference = 'Continue'
        & $exe quit *> $null
        $ErrorActionPreference = 'Stop'
    }
    # The exe stays locked for a moment after the app exits (Wait-Process returns
    # before that, #85), so the copy itself is retried until it goes through.
    $deadline = (Get-Date).AddSeconds(60)
    while ($true) {
        try {
            Copy-Item "$tmp\snowlearner-$target\snowlearner.exe" $exe -Force -ErrorAction Stop
            break
        }
        catch {
            if ((Get-Date) -gt $deadline) {
                Write-Host "Não deu para trocar $exe ($($_.Exception.Message))." -ForegroundColor Red
                Write-Host "Se o Snowlearner estiver aberto, feche-o (snowlearner quit ou pelo painel) e rode o instalador de novo." -ForegroundColor Red
                return
            }
            Start-Sleep -Milliseconds 500
        }
    }
    if ($open.Count -gt 0) { Write-Host "Snowlearner fechado para a atualização. Abra de novo quando terminar." }
    Write-Host "Instalado em $dir\snowlearner.exe"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' | Where-Object { $_ -eq $dir })) {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User')
        $env:Path = "$env:Path;$dir"
        Write-Host "Adicionado ao PATH (abra um novo terminal para usar 'snowlearner')."
    }

    $setupArgs = @('setup')
    if ($env:SNOWLEARNER_LANG) { $setupArgs += @('--lang', $env:SNOWLEARNER_LANG) }
    if ($env:SNOWLEARNER_NATIVE) { $setupArgs += @('--native', $env:SNOWLEARNER_NATIVE) }
    & "$dir\snowlearner.exe" @setupArgs

    # After setup: it writes the mage as snowlearner.ico next to the exe (render::icon::ICO_FILE).
    $startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Snowlearner.lnk'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($startMenu)
    $link.TargetPath = "$dir\snowlearner.exe"
    $link.Arguments = 'run'
    if (Test-Path "$dir\snowlearner.ico") { $link.IconLocation = "$dir\snowlearner.ico,0" }
    $link.Save()
    Write-Host ""
    Write-Host "Atalho criado no menu Iniciar."
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
