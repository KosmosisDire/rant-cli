#include "complete/shells.hpp"

#include <fstream>
#include <iterator>

#include "app/failure.hpp"
#include "process/command.hpp"
#include "util/home.hpp"

namespace complete {

namespace fs = std::filesystem;

/* bash splits words at = and :, so the part of a candidate up to the last of them is cut,
   since bash only replaces what follows. */
static const char* const bash_hook = R"HOOK(# rant tab completion for bash, written by `rant setup`
_rant_complete() {
    local line="${COMP_LINE:0:COMP_POINT}" word keep
    local -a words reply
    read -ra words <<< "$line"
    [[ $line == *[[:space:]] ]] && words+=("")
    word="${words[${#words[@]}-1]}"
    keep="${word%"${word##*[=:]}"}"
    local IFS=$'\n'
    reply=($(command rant __complete "--cur=$word" "${words[@]:1:${#words[@]}-2}" 2>/dev/null))
    COMPREPLY=("${reply[@]#"$keep"}")
    if [[ ${#reply[@]} -eq 1 && ${reply[0]} == *[/=] ]] && type compopt &>/dev/null; then
        compopt -o nospace
    fi
}
complete -F _rant_complete rant
)HOOK";

static const char* const zsh_hook = R"HOOK(# rant tab completion for zsh, written by `rant setup`
_rant() {
    local -a reply
    local c
    reply=("${(@f)$(command rant __complete "--cur=${words[CURRENT]}" "${(@)words[2,CURRENT-1]}" 2>/dev/null)}")
    for c in "${reply[@]}"; do
        [[ -z $c ]] && continue
        if [[ $c == *[/=] ]]; then compadd -S '' -- "$c"; else compadd -- "$c"; fi
    done
}
(( $+functions[compdef] )) || { autoload -Uz compinit && compinit }
compdef _rant rant
)HOOK";

/* fish adds no space after a candidate ending in / or = by itself. */
static const char* const fish_hook = R"HOOK(# rant tab completion for fish, written by `rant setup`
function __rant_complete
    set -l before (commandline -opc)
    set -l cur (commandline -ct)
    command rant __complete "--cur=$cur" $before[2..-1] 2>/dev/null
end
complete -c rant -f -a '(__rant_complete)'
)HOOK";

static const char* const powershell_hook = R"HOOK(# rant tab completion for PowerShell, written by `rant setup`
Register-ArgumentCompleter -Native -CommandName rant, rant.exe -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $words = @($commandAst.CommandElements | Select-Object -Skip 1 |
        Where-Object { $_.Extent.EndOffset -le $cursorPosition } | ForEach-Object { $_.Extent.Text })
    if ($wordToComplete -ne '' -and $words.Count -gt 0) { $words = @($words | Select-Object -SkipLast 1) }
    & rant __complete "--cur=$wordToComplete" @words 2>$null | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
    }
}
)HOOK";

const std::vector<Shell>& shells() {
    static const std::vector<Shell> list = {
        { "bash", bash_hook },
        { "zsh", zsh_hook },
        { "fish", fish_hook },
        { "powershell", powershell_hook },
        { "pwsh", powershell_hook },
    };
    return list;
}

const Shell* find_shell(std::string_view name) {
    for (auto& s : shells())
        if (s.name == name) return &s;
    return nullptr;
}

std::vector<const Shell*> detected() {
    std::vector<const Shell*> out;
#ifdef _WIN32
    const char* names[] = { "powershell", "pwsh" };
#else
    const char* names[] = { "bash", "zsh", "fish" };
#endif
    for (const char* n : names)
        if (process::find_program(n, fs::current_path())) out.push_back(find_shell(n));
    return out;
}

static fs::path home() {
    fs::path h = util::home_dir();
    if (h.empty()) throw app::Failure("no home folder known, set HOME");
    return h;
}

static void write_file(const fs::path& file, const std::string& text) {
    std::error_code ec;
    fs::create_directories(file.parent_path(), ec);
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    out << text;
    if (!out) throw app::Failure("cannot write " + file.u8string());
}

/* Adds line to a shell's startup file unless it is there already. */
static void add_line(const fs::path& rc, const std::string& line) {
    std::string text;
    {
        std::ifstream in(rc, std::ios::binary);
        text.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    if (text.find(line) != std::string::npos) return;
    std::ofstream out(rc, std::ios::binary | std::ios::app);
    if (!text.empty() && text.back() != '\n') out << "\n";
    out << line << "\n";
    if (!out) throw app::Failure("cannot write " + rc.u8string());
}

/* A path in single quotes for a POSIX shell. */
static std::string sh_quoted(const fs::path& p) {
    std::string s = p.generic_u8string(), out = "'";
    for (char c : s) out += c == '\'' ? std::string("'\\''") : std::string(1, c);
    return out + "'";
}

static fs::path hook_file(const std::string& name) {
    fs::path dir = util::rant_home();
    if (dir.empty()) throw app::Failure("no home folder known, set HOME");
    return dir / "completions" / name;
}

/* PowerShell edits its own profile, since only it knows where that is and whether it may
   run scripts. The script goes base64 encoded, which no quoting can break. */
static std::string encoded_command(const std::string& script) {
    static const char* b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::wstring wide = fs::u8path(script).wstring();
    std::string bytes;
    for (wchar_t c : wide) {
        bytes += (char)(c & 0xff);
        bytes += (char)((c >> 8) & 0xff);
    }
    std::string out;
    for (size_t i = 0; i < bytes.size(); i += 3) {
        uint32_t n = (uint8_t)bytes[i] << 16;
        if (i + 1 < bytes.size()) n |= (uint8_t)bytes[i + 1] << 8;
        if (i + 2 < bytes.size()) n |= (uint8_t)bytes[i + 2];
        out += b64[(n >> 18) & 63];
        out += b64[(n >> 12) & 63];
        out += i + 1 < bytes.size() ? b64[(n >> 6) & 63] : '=';
        out += i + 2 < bytes.size() ? b64[n & 63] : '=';
    }
    return out;
}

/* A string in single quotes for PowerShell. */
static std::string ps_quoted(const std::string& s) {
    std::string out = "'";
    for (char c : s) out += c == '\'' ? std::string("''") : std::string(1, c);
    return out + "'";
}

static std::string install_powershell(const Shell& shell, const fs::path& hook) {
    std::string path = ps_quoted(hook.u8string());
    std::string line = "if (Test-Path " + path + ") { . " + path + " }";
    std::string script =
        "$line = " + ps_quoted(line) + "\n"
        "$policy = Get-ExecutionPolicy\n"
        "if ($policy -eq 'Restricted' -or $policy -eq 'AllSigned') { exit 3 }\n"
        "New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null\n"
        "if (-not ((Test-Path $PROFILE) -and (Select-String -Path $PROFILE -SimpleMatch $line -Quiet))) {\n"
        "    Add-Content -Path $PROFILE -Value $line -Encoding utf8\n"
        "}\n"
        "exit 0\n";
    std::string exe(shell.name);
    if (!process::find_program(exe, fs::current_path())) throw app::Failure(exe + " is not on PATH");
    int code = process::run({ { exe, "-NoProfile", "-NoLogo", "-NonInteractive", "-EncodedCommand", encoded_command(script) },
                              fs::current_path(), {} });
    if (code == 3)
        throw app::Failure("it may not run scripts here, allow that with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` "
                           "in " + exe + " and run `rant setup " + exe + "` again");
    if (code != 0) throw app::Failure("could not edit its profile, " + exe + " exited " + std::to_string(code));
    return "loads from its profile";
}

std::string install(const Shell& shell) {
    if (shell.name == "fish") {
        const char* xdg = std::getenv("XDG_CONFIG_HOME");
        fs::path config = xdg && *xdg ? fs::u8path(xdg) : home() / ".config";
        fs::path file = config / "fish" / "completions" / "rant.fish";
        write_file(file, std::string(shell.hook));
        return "loads from " + file.u8string();
    }
    if (shell.name == "powershell" || shell.name == "pwsh") {
        fs::path hook = hook_file("rant.ps1");
        write_file(hook, std::string(shell.hook));
        return install_powershell(shell, hook);
    }
    fs::path hook = hook_file(shell.name == "bash" ? "rant.bash" : "rant.zsh");
    write_file(hook, std::string(shell.hook));
    fs::path rc;
    if (shell.name == "bash") {
        rc = home() / ".bashrc";
    } else {
        const char* zdot = std::getenv("ZDOTDIR");
        rc = (zdot && *zdot ? fs::u8path(zdot) : home()) / ".zshrc";
    }
    add_line(rc, "[ -f " + sh_quoted(hook) + " ] && . " + sh_quoted(hook));
    return "loads from " + rc.u8string();
}

}
