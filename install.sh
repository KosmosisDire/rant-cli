#!/bin/sh
# Installs rant-cli, the command line tool for Rant, on Linux and macOS:
#   curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh
# RANT_VERSION picks a release instead of the newest, RANT_HOME the folder instead of
# ~/.rant, and -y or RANT_YES=1 installs rant-cli alone without the checklist.
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

# the checklist needs a person at the terminal, which a pipe or -y says there is not
pick=""
[ -z "$yes" ] && has_tty && pick=1

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
[ -n "$pick" ] && printf '    %s-%s install what you tick below\n' "$faint" "$reset"
echo

# The checklist runs from a download outside the folder, so cancelling it leaves this machine
# as it was.
staging=$(mktemp -d) || fail "cannot make a temporary folder"
staged="$staging/rant"
unstage() { rm -f "$staged"; rmdir "$staging" 2> /dev/null || true; }
printf '  %sdownloading%s %s\n' "$dim" "$reset" "$url"
curl -fSL --progress-bar -o "$staged" "$url" || { unstage; fail "cannot download $url"; }
chmod +x "$staged"
"$staged" --version > /dev/null || { unstage; fail "the downloaded rant-cli does not run here"; }

if [ -n "$pick" ]; then
    echo
    code=0
    "$staged" install < /dev/tty || code=$?
    # 130 is the checklist cancelled
    if [ "$code" -eq 130 ]; then
        unstage
        printf '  nothing was installed\n\n'
        exit 1
    fi
    [ "$code" -ne 0 ] && warn "a package did not install, fix what rant said above, then run \`rant install <package>\`"
    echo
fi

mkdir -p "$bin"
mv -f "$staged" "$bin/rant"
unstage
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

# a script run through sh cannot change the shell that started it, so only a new one has
# rant on PATH and tab completion
echo
printf '%sDone.%s %s is installed.\n' "$green$bold" "$reset" "$("$bin/rant" --version)"
printf '  Open a new shell to use it, then try %srant --help%s.\n' "$bold" "$reset"
[ -z "$pick" ] && printf '  %srant install%s picks packages to add.\n' "$bold" "$reset"
echo
