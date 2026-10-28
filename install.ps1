# Installs rant-cli, the command line tool for Rant, on Windows:
#   irm https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.ps1 | iex
# RANT_VERSION picks a release instead of the newest, RANT_HOME the folder instead of
# ~\.rant, and RANT_YES=1 answers yes and skips the package list.
# It runs in the caller's session through iex, so everything stays in a scope of its own.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 may still default to a TLS that GitHub refuses
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    # its progress bar slows a download many times over
    $ProgressPreference = 'SilentlyContinue'

    $repo = 'KosmosisDire/rant-cli'
    $home_dir = if ($env:RANT_HOME) { $env:RANT_HOME } else { Join-Path $env:USERPROFILE '.rant' }
    $bin = Join-Path $home_dir 'bin'
    $exe = Join-Path $bin 'rant.exe'
    $yes = [bool]$env:RANT_YES

    # the console's own colors nearest the CLI's palette, none with NO_COLOR
    function Say($text, $color) {
        if ($env:NO_COLOR -or -not $color) { Write-Host -NoNewline $text }
        else { Write-Host -NoNewline $text -ForegroundColor $color }
    }
    function Step($text) { Say '  + ' 'Green'; Write-Host $text }
    function Warn($text) { Say '  ! ' 'Yellow'; Write-Host $text }
    function Fail($message) {
        Say '  x ' 'Red'; Write-Host $message
        throw $message
    }
    function Short($path) {
        if ($path.StartsWith($env:USERPROFILE)) { '~' + $path.Substring($env:USERPROFILE.Length) } else { $path }
    }
    # Yes when answered yes or left empty, and at once with RANT_YES.
    function Ask($question) {
        if ($yes) { return $true }
        Say '? ' 'Red'; Say "$question " 'White'; Say '[Y/n] ' 'DarkGray'
        $answer = Read-Host
        Write-Host ''
        return $answer -in @('', 'y', 'Y', 'yes', 'Yes')
    }
    # A native command's output, stderr included, as text lines, and whether it succeeded.
    function Capture([scriptblock]$command) {
        $ErrorActionPreference = 'Continue'    # 5.1 turns redirected stderr into errors
        $lines = & $command 2>&1 | ForEach-Object { "$_" }
        return @{ ok = $LASTEXITCODE -eq 0; lines = $lines }
    }

    Write-Host ''
    Say 'rant' 'Red'; Write-Host '-cli installer'
    Write-Host ''

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

    # the version already there, from `rant-cli 0.0.32 (Rant 0.0.36)`
    $had = ''
    if (Test-Path $exe) {
        $was = (Capture { & $exe --version }).lines -join ''
        if ($was -match '^rant-cli ([0-9.]+)') {
            $had = if ($Matches[1] -eq $version) { 'installed already, installs it again' } else { "replaces $($Matches[1])" }
        }
    }

    $user_path = [Environment]::GetEnvironmentVariable('Path', 'User')
    $on_path = ($user_path -split ';') -contains $bin

    Say '  version   ' 'DarkGray'; Write-Host -NoNewline $version
    if ($had) { Say "  $had" 'DarkGray' }
    Write-Host ''
    Say '  platform  ' 'DarkGray'; Write-Host 'win-x64'
    Say '  folder    ' 'DarkGray'; Write-Host (Short $bin)
    Write-Host ''
    Write-Host '  It will:'
    Say '    - ' 'DarkGray'; Write-Host "download rant-cli to $(Short $exe)"
    if (-not $on_path) { Say '    - ' 'DarkGray'; Write-Host "put $(Short $bin) on your user PATH" }
    Say '    - ' 'DarkGray'; Write-Host 'set up tab completion for PowerShell'
    if (-not $yes) { Say '    - ' 'DarkGray'; Write-Host 'let you pick packages to install' }
    Write-Host ''

    if (-not (Ask 'Install?')) {
        Write-Host '  nothing was installed'
        return
    }

    New-Item -ItemType Directory -Force $bin | Out-Null
    $part = "$exe.part"
    Say '  downloading ' 'DarkGray'; Write-Host $url
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
    if (-not (Capture { & $exe --version }).ok) { Fail 'the downloaded rant-cli does not run here' }
    Step "installed $(Short $exe)"

    if (-not $on_path) {
        $new_path = if ($user_path) { "$bin;$user_path" } else { $bin }
        [Environment]::SetEnvironmentVariable('Path', $new_path, 'User')
        Step "$(Short $bin) is on your user PATH"
    }
    if (($env:Path -split ';') -notcontains $bin) { $env:Path = "$bin;$env:Path" }

    $setup = Capture { & $exe setup }
    if ($setup.ok) { Step 'tab completion' }
    else { Warn 'tab completion is not set up, fix this, then run `rant setup`:' }
    foreach ($line in $setup.lines) { Say "      $line" 'DarkGray'; Write-Host '' }
    # iex runs this in the caller's session, so its completer works there at once. As text it
    # loads even where the execution policy refuses script files, and so the profile.
    $hook = Join-Path $home_dir 'completions\rant.ps1'
    if (Test-Path $hook) { Invoke-Expression (Get-Content -Raw $hook) }

    if (-not $yes) {
        Write-Host ''
        & $exe install
        if ($LASTEXITCODE -ne 0) { Warn 'a package did not install, fix what rant said above, then run `rant install <package>`' }
    }

    Write-Host ''
    Say 'Done. ' 'Green'; Write-Host "$((Capture { & $exe --version }).lines -join '') is installed."
    Write-Host '  This window can use it now, other open ones once restarted. Try ' -NoNewline
    Say 'rant --help' 'White'; Write-Host '.'
    if ($yes) { Write-Host '  ' -NoNewline; Say 'rant install' 'White'; Write-Host ' picks packages to add.' }
    Write-Host ''
}
