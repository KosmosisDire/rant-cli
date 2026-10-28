#!/bin/sh
# Installs rant-cli, the command line tool for Rant, on Linux and macOS:
#   curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh
# RANT_VERSION picks a release instead of the newest, RANT_HOME the folder instead of
# ~/.rant, and -y or RANT_YES=1 answers yes and skips the package list.
set -eu

repo="KosmosisDire/rant-cli"
home="${RANT_HOME:-$HOME/.rant}"
bin="$home/bin"
yes="${RANT_YES:-}"
for arg in "$@"; do
    case "$arg" in
        -y | --yes) yes=1 ;;
        *) echo "unknown option $arg" >&2; exit 2 ;;
    esac
done

# the CLI's palette, only on a terminal and never with NO_COLOR
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    esc=$(printf '\033')
    bold="$esc[1m" dim="$esc[38;2;154;154;154m" faint="$esc[38;2;100;100;100m"
    accent="$esc[38;2;220;56;83m" green="$esc[38;2;92;166;125m" amber="$esc[38;2;207;154;58m"
    red="$esc[38;2;210;87;75m" reset="$esc[0m"
else
    bold="" dim="" faint="" accent="" green="" amber="" red="" reset=""
fi

step() { printf '  %s+%s %s\n' "$green" "$reset" "$*"; }
warn() { printf '  %s!%s %s\n' "$amber" "$reset" "$*" >&2; }
fail() {
    printf '  %sx%s %s\n' "$red" "$reset" "$*" >&2
    exit 1
}
# ~ for the home folder, so paths read short
short() { case "$1" in "$HOME"/*) echo "~${1#"$HOME"}" ;; *) echo "$1" ;; esac; }

# stdin is the script itself when it comes through a pipe, so questions go to the terminal
has_tty() { [ -r /dev/tty ] && { : < /dev/tty; } 2> /dev/null; }

# Yes when answered yes or left empty, and at once with -y or with no terminal to ask.
ask() {
    if [ -n "$yes" ] || ! has_tty; then return 0; fi
    printf '%s?%s %s%s%s %s[Y/n]%s ' "$accent" "$reset" "$bold" "$1" "$reset" "$faint" "$reset"
    read -r answer < /dev/tty || answer=""
    echo
    case "$answer" in
        "" | y | Y | yes | Yes) return 0 ;;
        *) return 1 ;;
    esac
}

printf '\n%srant%s%s-cli installer%s\n\n' "$accent$bold" "$reset" "$bold" "$reset"

command -v curl > /dev/null 2>&1 || fail "the install needs curl"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64 | Linux-amd64) platform="linux-x64" ;;
    Darwin-arm64) platform="osx-arm64" ;;
    *) fail "there is no rant-cli build for $(uname -s) on $(uname -m)" ;;
esac

# The newest release is where GitHub's latest link leads, which needs no API call.
version="${RANT_VERSION:-}"
if [ -z "$version" ]; then
    latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest") ||
        fail "cannot reach GitHub"
    version="${latest##*/}"
fi
version="${version#v}"
case "$version" in
    "" | *[!0-9.]*) fail "found no rant-cli release" ;;
esac
url="https://github.com/$repo/releases/download/v$version/rant-$version-$platform"
# the version already there, from `rant-cli 0.0.32 (Rant 0.0.36)`
had=""
[ -x "$bin/rant" ] && had=$("$bin/rant" --version 2> /dev/null | sed -n 's/^rant-cli \([0-9.]*\).*/\1/p' || true)
case "$had" in
    "") ;;
    "$version") had="installed already, installs it again" ;;
    *) had="replaces $had" ;;
esac

# The startup files that put the folder on PATH: those of bash, zsh and fish when found,
# else ~/.profile.
rcs=""
command -v bash > /dev/null 2>&1 && rcs="$rcs $HOME/.bashrc"
command -v zsh > /dev/null 2>&1 && rcs="$rcs ${ZDOTDIR:-$HOME}/.zshrc"
[ -z "$rcs" ] && rcs="$HOME/.profile"
fish_conf=""
command -v fish > /dev/null 2>&1 && fish_conf="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/rant.fish"
rc_names=""
for rc in $rcs; do rc_names="$rc_names${rc_names:+, }$(short "$rc")"; done
[ -n "$fish_conf" ] && rc_names="$rc_names, $(short "$fish_conf")"

printf '  %sversion%s   %s%s\n' "$dim" "$reset" "$version" "${had:+  $faint$had$reset}"
printf '  %splatform%s  %s\n' "$dim" "$reset" "$platform"
printf '  %sfolder%s    %s\n\n' "$dim" "$reset" "$(short "$bin")"
printf '  It will:\n'
printf '    %s-%s download rant-cli to %s\n' "$faint" "$reset" "$(short "$bin/rant")"
printf '    %s-%s put %s on PATH in %s\n' "$faint" "$reset" "$(short "$bin")" "$rc_names"
printf '    %s-%s set up tab completion for your shells\n' "$faint" "$reset"
[ -z "$yes" ] && has_tty && printf '    %s-%s let you pick packages to install\n' "$faint" "$reset"
echo

if ! ask "Install?"; then
    echo "  nothing was installed"
    exit 1
fi

mkdir -p "$bin"
printf '  %sdownloading%s %s\n' "$dim" "$reset" "$url"
curl -fSL --progress-bar -o "$bin/rant.part" "$url" || fail "cannot download $url"
chmod +x "$bin/rant.part"
mv -f "$bin/rant.part" "$bin/rant"
"$bin/rant" --version > /dev/null || fail "the downloaded rant-cli does not run here"
step "installed $(short "$bin/rant")"

line="export PATH=\"$bin:\$PATH\"    # rant-cli"
for rc in $rcs; do
    if ! grep -qsF "$line" "$rc"; then
        printf '\n%s\n' "$line" >> "$rc"
    fi
done
if [ -n "$fish_conf" ]; then
    mkdir -p "$(dirname "$fish_conf")"
    echo "fish_add_path -g \"$bin\"    # rant-cli" > "$fish_conf"
fi
step "PATH set in $rc_names"

if setup=$(PATH="$bin:$PATH" "$bin/rant" setup 2>&1); then
    step "tab completion"
else
    warn "tab completion is not set up, fix this, then run \`rant setup\`:"
fi
printf '%s\n' "$setup" | sed "s/^/      $faint/; s/\$/$reset/"

# the picker needs a person at the terminal, which a pipe or -y says there is not
if [ -z "$yes" ] && has_tty; then
    echo
    PATH="$bin:$PATH" "$bin/rant" install < /dev/tty ||
        warn "a package did not install, fix what rant said above, then run \`rant install <package>\`"
fi

# a script run through sh cannot change the shell that started it, so only a new one has
# rant on PATH and tab completion
echo
printf '%sDone.%s %s is installed.\n' "$green$bold" "$reset" "$("$bin/rant" --version)"
printf '  Open a new shell to use it, then try %srant --help%s.\n' "$bold" "$reset"
[ -n "$yes" ] && printf '  %srant install%s picks packages to add.\n' "$bold" "$reset"
echo
