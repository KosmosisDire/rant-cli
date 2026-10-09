# Installs rant-cli, the command line tool for Rant, on Windows:
#   irm https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.ps1 | iex
# RANT_VERSION picks a release instead of the newest, RANT_HOME the folder instead of
# ~\.rant, and RANT_YES=1 answers yes.
# It runs in the caller's session through iex, so everything stays in a scope of its own.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 may still default to a TLS that GitHub refuses
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'KosmosisDire/rant-cli'
    $home_dir = if ($env:RANT_HOME) { $env:RANT_HOME } else { Join-Path $env:USERPROFILE '.rant' }
    $bin = Join-Path $home_dir 'bin'
    $exe = Join-Path $bin 'rant.exe'

    function Fail($message) {
        Write-Host "error: $message" -ForegroundColor Red
        throw $message
    }

    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { Fail "there is no rant-cli build for Windows on $env:PROCESSOR_ARCHITECTURE" }

    $version = $env:RANT_VERSION
    if (-not $version) {
        try {
            $version = (Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$repo/releases/latest").tag_name
        } catch {
            Fail "cannot reach GitHub: $($_.Exception.Message)"
        }
    }
    $version = $version.TrimStart('v')
    if ($version -notmatch '^[0-9.]+$') { Fail 'found no rant-cli release' }
    $url = "https://github.com/$repo/releases/download/v$version/rant-$version-win-x64.exe"

    $user_path = [Environment]::GetEnvironmentVariable('Path', 'User')
    $on_path = ($user_path -split ';') -contains $bin

    Write-Host "This installs rant-cli $version for win-x64:"
    Write-Host "  download $url"
    Write-Host "    to $exe"
    if (-not $on_path) { Write-Host "  add $bin to your user PATH" }
    Write-Host '  run rant setup, which sets up tab completion for PowerShell'

    if (-not $env:RANT_YES) {
        $answer = Read-Host 'Install? [y/N]'
        if ($answer -notin @('y', 'Y', 'yes')) {
            Write-Host 'nothing was installed'
            return
        }
    }

    New-Item -ItemType Directory -Force $bin | Out-Null
    $part = "$exe.part"
    try {
        Invoke-WebRequest -UseBasicParsing $url -OutFile $part
    } catch {
        Fail "cannot download ${url}: $($_.Exception.Message)"
    }
    # a running rant.exe cannot be replaced but can be renamed out of the way
    if (Test-Path $exe) {
        Remove-Item "$exe.old" -Force -ErrorAction SilentlyContinue
        Move-Item -Force $exe "$exe.old"
    }
    Move-Item -Force $part $exe
    & $exe --version | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail 'the downloaded rant-cli does not run here' }

    if (-not $on_path) {
        $new_path = if ($user_path) { "$bin;$user_path" } else { $bin }
        [Environment]::SetEnvironmentVariable('Path', $new_path, 'User')
    }
    if (($env:Path -split ';') -notcontains $bin) { $env:Path = "$bin;$env:Path" }

    & $exe setup
    if ($LASTEXITCODE -ne 0) { Write-Host 'warning: rant setup did not finish, run it again later' -ForegroundColor Yellow }

    Write-Host ''
    Write-Host "Installed $(& $exe --version). Open a new terminal to use it everywhere."
}
