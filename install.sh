#!/bin/sh
# Installs rant-cli, the command line tool for Rant, on Linux and macOS:
#   curl -fsSL https://raw.githubusercontent.com/KosmosisDire/rant-cli/main/install.sh | sh
# RANT_VERSION picks a release instead of the newest, RANT_HOME the folder instead of
# ~/.rant, and -y or RANT_YES=1 answers yes.
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

fail() {
    echo "error: $*" >&2
    exit 1
}

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

# The startup files that put the folder on PATH: those of bash, zsh and fish when found,
# else ~/.profile.
rcs=""
command -v bash > /dev/null 2>&1 && rcs="$rcs $HOME/.bashrc"
command -v zsh > /dev/null 2>&1 && rcs="$rcs ${ZDOTDIR:-$HOME}/.zshrc"
[ -z "$rcs" ] && rcs="$HOME/.profile"
fish_conf=""
command -v fish > /dev/null 2>&1 && fish_conf="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/rant.fish"
Claude Code|$HOME/.claude
Codex|$HOME/.codex
Cursor|$HOME/.cursor
Gemini CLI|$HOME/.gemini
GitHub Copilot|$HOME/.copilot
OpenCode|${XDG_CONFIG_HOME:-$HOME/.config}/opencode
EOF

echo "This installs rant-cli $version for $platform:"
echo "  download $url"
echo "    to $bin/rant"
for rc in $rcs; do echo "  add $bin to PATH in $rc"; done
[ -n "$fish_conf" ] && echo "  add $bin to PATH for fish in $fish_conf"
echo "  run rant setup, which sets up tab completion for your shells"

if [ -z "$yes" ]; then
    # stdin is the script itself when it comes through a pipe, so the answer comes from the terminal
    if [ -r /dev/tty ] && { : < /dev/tty; } 2> /dev/null; then
        printf "Install? [y/N] "
        read -r answer < /dev/tty || answer=""
        case "$answer" in
            y | Y | yes) ;;
            *) echo "nothing was installed"; exit 1 ;;
        esac
    else
        echo "Install? [y/N] y (no terminal to ask)"
    fi
fi

mkdir -p "$bin"
curl -fSL --progress-bar -o "$bin/rant.part" "$url" || fail "cannot download $url"
chmod +x "$bin/rant.part"
mv -f "$bin/rant.part" "$bin/rant"
"$bin/rant" --version > /dev/null || fail "the downloaded rant-cli does not run here"

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

PATH="$bin:$PATH" "$bin/rant" setup || echo "warning: rant setup did not finish, run it again later" >&2

echo
echo "Installed $("$bin/rant" --version). Open a new shell to use it, or run:"
echo "  export PATH=\"$bin:\$PATH\""
