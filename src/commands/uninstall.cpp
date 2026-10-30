#include <algorithm>
#include <chrono>
#include <fstream>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/shells.hpp"
#include "library/library.hpp"
#include "packages/packages.hpp"
#include "packages/vsix.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "services/entry.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cwctype>
#endif

namespace commands {

namespace fs = std::filesystem;
using complete::Added;

/* The bridge package's config, which holds the user's passphrase and settings. */
static fs::path bridge_config() { return util::rant_home() / "bridge.yaml"; }

#ifdef _WIN32
/* A path as the user PATH compares it: normalized, without a trailing slash, any case. */
static std::wstring folded(const std::wstring& s) {
    std::wstring w = fs::path(s).lexically_normal().make_preferred().wstring();
    while (w.size() > 3 && w.back() == L'\\') w.pop_back();
    for (auto& c : w) c = (wchar_t)std::towlower(c);
    return w;
}

/* The user PATH as written, %VARS% unexpanded, with its value type. */
static bool read_user_path(std::wstring& value, DWORD& type) {
    HKEY key;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Environment", 0, KEY_READ, &key) != ERROR_SUCCESS) return false;
    DWORD size = 0;
    bool ok = RegQueryValueExW(key, L"Path", nullptr, &type, nullptr, &size) == ERROR_SUCCESS;
    if (ok) {
        value.assign(size / sizeof(wchar_t), L'\0');
        ok = RegQueryValueExW(key, L"Path", nullptr, &type, (BYTE*)value.data(), &size) == ERROR_SUCCESS;
        while (!value.empty() && value.back() == L'\0') value.pop_back();
    }
    RegCloseKey(key);
    return ok;
}

/* The bin folder on the user PATH, where install.ps1 puts it. */
static void user_path(std::vector<Added>& out, const fs::path& bin) {
    std::wstring value;
    DWORD type = REG_EXPAND_SZ;
    if (!read_user_path(value, type)) return;
    std::wstring kept, mine = folded(bin.wstring());
    bool found = false;
    for (size_t at = 0; at <= value.size();) {
        size_t end = value.find(L';', at);
        if (end == std::wstring::npos) end = value.size();
        std::wstring entry = value.substr(at, end - at);
        if (!entry.empty() && folded(entry) == mine) found = true;
        else if (!entry.empty()) kept += (kept.empty() ? L"" : L";") + entry;
        at = end + 1;
    }
    if (!found) return;
    out.push_back({ bin, "on your user PATH", [kept, type] {
                       HKEY key;
                       if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Environment", 0, KEY_SET_VALUE, &key) != ERROR_SUCCESS)
                           throw app::Failure("cannot change your user PATH");
                       LONG r = RegSetValueExW(key, L"Path", 0, type, (const BYTE*)kept.c_str(), (DWORD)((kept.size() + 1) * sizeof(wchar_t)));
                       RegCloseKey(key);
                       if (r != ERROR_SUCCESS) throw app::Failure("cannot change your user PATH");
                       /* open programs such as Explorer read the change from this message */
                       SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, (LPARAM)L"Environment", SMTO_ABORTIFHUNG, 2000, nullptr);
                   } });
}

/* What remove_all could not delete, a running program such as this rant or an open
 * explorer, goes once this process has ended, by a script that waits for it a minute. */
static void remove_later(const fs::path& home) {
    std::vector<fs::path> files, dirs;
    std::error_code ec;
    for (fs::recursive_directory_iterator it(home, ec), end; !ec && it != end; it.increment(ec)) {
        if (it->path() == bridge_config()) continue;
        (it->is_directory(ec) ? dirs : files).push_back(it->path());
    }
    std::sort(dirs.begin(), dirs.end(), [](const fs::path& a, const fs::path& b) { return a.native().size() > b.native().size(); });
    dirs.push_back(home);

    fs::path script = fs::temp_directory_path() / ("rant-uninstall-" + std::to_string(GetCurrentProcessId()) + ".cmd");
    std::ofstream s(script, std::ios::binary);
    s << "@echo off\r\nchcp 65001 >nul\r\nset n=0\r\n:again\r\n";
    for (auto& f : files) s << "del /f /q \"" << f.u8string() << "\" 2>nul\r\n";
    for (auto& f : files) s << "if exist \"" << f.u8string() << "\" goto wait\r\n";
    s << "goto dirs\r\n:wait\r\nset /a n+=1\r\nif %n% geq 60 goto dirs\r\nping -n 2 127.0.0.1 >nul\r\ngoto again\r\n:dirs\r\n";
    for (auto& d : dirs) s << "rmdir \"" << d.u8string() << "\" 2>nul\r\n";
    s << "(goto) 2>nul & del \"%~f0\"\r\n";
    s.close();
    if (!s) throw app::Failure("cannot write " + script.u8string());
    std::string job = "Local\\rant-uninstall-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    process::start_detached({ { "cmd.exe", "/d", "/c", script.u8string() }, fs::temp_directory_path(), {} }, "NUL", job);
}
#endif

/* What ~/.rant holds but the bridge config, which keeps the user's passphrase and settings. */
static std::vector<fs::path> removable(const fs::path& home) {
    std::vector<fs::path> out;
    std::error_code ec;
    for (fs::directory_iterator it(home, ec), end; !ec && it != end; it.increment(ec))
        if (it->path() != bridge_config()) out.push_back(it->path());
    return out;
}

/* ~/.rant and all in it but the bridge config: this rant, the installed packages, the shell
 * hooks, caches, the workspace list and the NuGet feed. */
static void remove_home(const fs::path& home) {
    packages::unlink_all();
    std::error_code ec;
    for (auto& p : removable(home)) fs::remove_all(p, ec);
    if (removable(home).empty()) {
        fs::remove(home, ec);    /* stays when the config is in it */
        return;
    }
#ifdef _WIN32
    remove_later(home);
#else
    throw app::Failure("cannot remove " + home.u8string() + ": " + ec.message());
#endif
}

static int run(app::Context& ctx) {
    if (!ctx.args.words.empty()) throw app::UsageError("uninstall takes no arguments");
    fs::path home = util::rant_home();
    if (home.empty()) throw app::Failure("no home folder known, set HOME");
    /* RANT_HOME may point anywhere, and only a folder of rant's own is deleted whole */
    if (home.filename() != ".rant")
        throw app::Failure("RANT_HOME is " + home.u8string() + ", uninstall only removes a folder named .rant");

    /* the folders the installers changed, then what setup added, then ~/.rant itself */
    std::vector<Added> steps = complete::added();
#ifdef _WIN32
    user_path(steps, home / "bin");
#endif
    if (library::nuget_feed_added())
        steps.push_back({ {}, "the NuGet source rant", [] {
                             if (!process::find_program("dotnet", fs::temp_directory_path()))
                                 throw app::Failure("dotnet not found, remove the NuGet source rant by hand");
                             if (process::run({ { "dotnet", "nuget", "remove", "source", "rant" }, fs::temp_directory_path(), {} }) != 0)
                                 throw app::Failure("dotnet could not remove the NuGet source rant");
                         } });
    for (auto& name : packages::installed()) {
        std::optional<packages::Record> r = packages::record(name);
        if (!r) continue;
        for (auto& link : r->skills)
            if (packages::is_link(link)) steps.push_back({ link, "the " + name + " skill", [link] { packages::unlink(link); } });
        if (r->extension.empty() || !fs::is_regular_file(r->editor)) continue;
        packages::vsix::Target t{ r->editor, r->profile };
        if (!packages::vsix::installed_version(t, r->extension).empty())
            steps.push_back({ {}, "the VS Code extension " + r->extension + ", through " + packages::vsix::shown(t),
                              [t, id = r->extension] { packages::vsix::remove(t, id); } });
    }
    if (services::entry::present(services::entry::When::Login))
        steps.push_back({ {}, "the login entry that starts what rant enable named", [] { services::entry::remove(services::entry::When::Login); } });
    if (services::entry::present(services::entry::When::Boot))
        steps.push_back({ {}, services::entry::boot_place() + ", which asks for admin",
                          [] { services::entry::remove(services::entry::When::Boot); } });
    std::error_code ec;
    bool config_only = fs::exists(bridge_config(), ec) && removable(home).empty();
    if (fs::exists(home, ec) && !config_only) steps.push_back({ home, "", [home] { remove_home(home); } });

    if (steps.empty()) {
        ctx.out.line("rant-cli left nothing here to remove");
        return 0;
    }
    ctx.out.line("This uninstalls rant-cli and what it installed, removing:");
    for (auto& s : steps) {
        std::string where = fs::path(s.path).lexically_normal().make_preferred().u8string();
        ctx.out.line("  " + (s.path.empty() ? s.part : s.part.empty() ? where : where + " (" + s.part + ")"));
    }
    ctx.out.line("Workspaces stay as they are, and running nodes keep running.");
    if (fs::exists(bridge_config(), ec)) ctx.out.line(bridge_config().u8string() + " stays, it holds your bridge passphrase and settings.");
    if (!ui::confirm("Uninstall?", ctx.yes)) return 1;
    bool ok = true;
    for (auto& s : steps) {
        try {
            s.remove();
        } catch (const app::Failure& e) {
            ctx.out.warn(e.what());
            ok = false;
        }
    }
    ctx.out.line(ok ? "uninstalled rant-cli" : "uninstalled rant-cli, except what the warnings name");
    return ok ? 0 : 1;
}

app::Command uninstall() {
    return { "uninstall", "", "remove rant-cli, the installed packages and what setup added, never touching workspaces",
             app::Section::Setup, {}, run };
}

}
