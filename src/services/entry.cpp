#include "services/entry.hpp"

#include <fstream>
#include <iterator>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "process/quote.hpp"
#include "process/supervisor.hpp"
#include "services/services.hpp"
#include "util/home.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <unistd.h>
#endif

namespace services::entry {

/* A path as a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

std::vector<std::string> argv(const fs::path& rant, const fs::path& home) {
    return { native(rant), "__services", "--login", native(home) };
}

static std::vector<std::string> mine() { return argv(process::self_path(), util::rant_home()); }

/* A word as systemd reads it inside ExecStart: quoted, with its own escapes and % and $ doubled. */
static std::string systemd_word(const std::string& s) {
    std::string out = "\"";
    for (char c : s) {
        if (c == '\\' || c == '"') out += '\\';
        if (c == '%' || c == '$') out += c;
        out += c;
    }
    return out + "\"";
}

/* Only the service process is stopped with the unit, so the nodes it started keep running. */
std::string systemd_unit(const std::vector<std::string>& argv) {
    std::string exec;
    for (auto& a : argv) exec += (exec.empty() ? "" : " ") + systemd_word(a);
    return "[Unit]\n"
           "Description=Rant service process\n"
           "\n"
           "[Service]\n"
           "ExecStart=" + exec + "\n"
           "KillMode=process\n"
           "\n"
           "[Install]\n"
           "WantedBy=default.target\n";
}

static std::string xml_text(const std::string& s) {
    std::string out;
    for (char c : s) {
        if (c == '&') out += "&amp;";
        else if (c == '<') out += "&lt;";
        else if (c == '>') out += "&gt;";
        else out += c;
    }
    return out;
}

/* It ends when it has nothing to watch, so launchd neither keeps it alive nor takes its nodes. */
std::string launchd_plist(const std::vector<std::string>& argv, const fs::path& log) {
    auto str = [](const std::string& v) { return "<string>" + xml_text(v) + "</string>"; };
    std::string args;
    for (auto& a : argv) args += "    " + str(a) + "\n";
    return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
           "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
           "<plist version=\"1.0\">\n"
           "<dict>\n"
           "  <key>Label</key>\n"
           "  <string>dev.rantlib.rant</string>\n"
           "  <key>ProgramArguments</key>\n"
           "  <array>\n" +
           args +
           "  </array>\n"
           "  <key>RunAtLoad</key>\n"
           "  <true/>\n"
           "  <key>AbandonProcessGroup</key>\n"
           "  <true/>\n"
           "  <key>StandardOutPath</key>\n"
           "  " + str(native(log)) + "\n"
           "  <key>StandardErrorPath</key>\n"
           "  " + str(native(log)) + "\n"
           "</dict>\n"
           "</plist>\n";
}

#ifdef _WIN32

static const wchar_t* run_key = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
static const wchar_t* run_name = L"Rant";

static std::wstring run_value() {
    DWORD size = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, run_name, RRF_RT_REG_SZ, nullptr, nullptr, &size) != ERROR_SUCCESS) return {};
    std::wstring v(size / sizeof(wchar_t), L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, run_name, RRF_RT_REG_SZ, nullptr, v.data(), &size) != ERROR_SUCCESS) return {};
    v.resize(wcslen(v.c_str()));
    return v;
}

static std::wstring wanted() { return config::from_utf8(process::windows_command_line(mine())).wstring(); }

bool present() { return run_value() == wanted(); }

void write() {
    std::wstring v = wanted();
    if (RegSetKeyValueW(HKEY_CURRENT_USER, run_key, run_name, REG_SZ, v.c_str(), (DWORD)((v.size() + 1) * sizeof(wchar_t))) != ERROR_SUCCESS)
        throw app::Failure("cannot write the Run key of your user, so nothing can start at login");
}

void remove() {
    if (present()) RegDeleteKeyValueW(HKEY_CURRENT_USER, run_key, run_name);
}

#else

static fs::path file() {
#ifdef __APPLE__
    return util::home_dir() / "Library" / "LaunchAgents" / "dev.rantlib.rant.plist";
#else
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = util::home_dir() / ".config";
    return config / "systemd" / "user" / "rant.service";
#endif
}

static std::string text() {
#ifdef __APPLE__
    return launchd_plist(mine(), log_file());
#else
    return systemd_unit(mine());
#endif
}

bool present() {
    std::ifstream in(file(), std::ios::binary);
    return in && std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()) == text();
}

/* A service manager command. A failure is the user's to read, so its output stays on screen. */
static void manage(const std::vector<std::string>& argv, bool must = true) {
    if (process::run({ argv, fs::temp_directory_path(), {} }) != 0 && must)
        throw app::Failure("`" + process::shown(argv) + "` failed, see what it said above");
}

#ifdef __APPLE__
static std::string agent() { return "gui/" + std::to_string(getuid()) + "/dev.rantlib.rant"; }
#endif

void write() {
    if (present()) return;
    std::error_code ec;
    fs::create_directories(file().parent_path(), ec);
    std::ofstream(file(), std::ios::binary) << text();
    if (!present()) throw app::Failure("cannot write " + file().u8string() + ", so nothing can start at login");
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
    manage({ "launchctl", "bootstrap", "gui/" + std::to_string(getuid()), native(file()) });
#else
    manage({ "systemctl", "--user", "daemon-reload" });
    manage({ "systemctl", "--user", "enable", "rant.service" });
#endif
}

void remove() {
    if (!present()) return;
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
#else
    manage({ "systemctl", "--user", "disable", "rant.service" }, false);
#endif
    std::error_code ec;
    fs::remove(file(), ec);
#ifndef __APPLE__
    manage({ "systemctl", "--user", "daemon-reload" }, false);
#endif
}

#endif

}
