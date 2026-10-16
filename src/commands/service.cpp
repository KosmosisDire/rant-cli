#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <thread>

#include "app/failure.hpp"
#include "commands/service.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "process/quote.hpp"
#include "process/supervisor.hpp"
#include "run/logs.hpp"
#include "util/home.hpp"
#include "util/interrupt.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <unistd.h>
#endif

namespace commands::service {

namespace fs = std::filesystem;

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

/* A path as a program or a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

std::optional<State> parse_state(const std::string& text) {
    State s;
    std::istringstream in(text);
    for (std::string line; std::getline(in, line);) {
        size_t colon = line.find(':');
        if (colon == std::string::npos) continue;
        std::string key = line.substr(0, colon), value = line.substr(colon + 1);
        value.erase(0, value.find_first_not_of(" \t"));
        value.erase(value.find_last_not_of(" \t\r") + 1);
        if (key == "pid") s.pid = std::strtoull(value.c_str(), nullptr, 10);
        else if (key == "url") s.url = value;
        else if (key == "version") s.version = value;
        else if (key == "started") s.started = std::strtoll(value.c_str(), nullptr, 10);
    }
    if (s.pid == 0) return std::nullopt;
    return s;
}

std::optional<State> running(const Service& s) {
    std::ifstream in(s.state);
    std::optional<State> st = parse_state(std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()));
    if (st && process::pid_alive(st->pid)) return st;
    return std::nullopt;
}

std::string installed_version(const Service& s) {
    std::ifstream in(s.version_file);
    std::string v;
    std::getline(in, v);
    return v;
}

std::vector<std::string> windows_argv(const Service& s) {
    std::vector<std::string> argv = { native(s.exe) };
    argv.insert(argv.end(), s.windows_args.begin(), s.windows_args.end());
    return argv;
}

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

std::string systemd_unit(const Service& s) {
    std::string exec = systemd_word(native(s.exe));
    for (const auto& a : s.args) exec += " " + systemd_word(a);
    return "[Unit]\n"
           "Description=" + s.description + "\n"
           "\n"
           "[Service]\n"
           "ExecStart=" + exec + "\n"
           "Restart=on-failure\n"
           "RestartSec=1\n"
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

std::string launchd_plist(const Service& s) {
    auto str = [](const std::string& v) { return "<string>" + xml_text(v) + "</string>"; };
    std::string args = "    " + str(native(s.exe)) + "\n";
    for (const auto& a : s.args) args += "    " + str(a) + "\n";
    return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
           "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
           "<plist version=\"1.0\">\n"
           "<dict>\n"
           "  <key>Label</key>\n"
           "  " + str(s.label) + "\n"
           "  <key>ProgramArguments</key>\n"
           "  <array>\n" +
           args +
           "  </array>\n"
           "  <key>RunAtLoad</key>\n"
           "  <true/>\n"
           "  <key>KeepAlive</key>\n"
           "  <dict>\n"
           "    <key>SuccessfulExit</key>\n"
           "    <false/>\n"
           "  </dict>\n"
           "  <key>StandardOutPath</key>\n"
           "  " + str(native(s.log)) + "\n"
           "  <key>StandardErrorPath</key>\n"
           "  " + str(native(s.log)) + "\n"
           "</dict>\n"
           "</plist>\n";
}

#ifdef _WIN32

static const wchar_t* run_key = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";

static std::wstring run_value(const Service& s) {
    std::wstring name = config::from_utf8(s.run_name).wstring();
    DWORD size = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, name.c_str(), RRF_RT_REG_SZ, nullptr, nullptr, &size) != ERROR_SUCCESS) return {};
    std::wstring v(size / sizeof(wchar_t), L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, name.c_str(), RRF_RT_REG_SZ, nullptr, v.data(), &size) != ERROR_SUCCESS) return {};
    v.resize(wcslen(v.c_str()));
    return v;
}

/* Another home's program, such as the real one while a test runs, is not this one. */
bool enabled(const Service& s) { return run_value(s).find(s.exe.wstring()) != std::wstring::npos; }

void define(const Service& s) {
    std::wstring name = config::from_utf8(s.run_name).wstring();
    std::wstring v = config::from_utf8(process::windows_command_line(windows_argv(s))).wstring();
    if (RegSetKeyValueW(HKEY_CURRENT_USER, run_key, name.c_str(), REG_SZ, v.c_str(), (DWORD)((v.size() + 1) * sizeof(wchar_t))) != ERROR_SUCCESS)
        throw app::Failure("cannot write the Run key of your user, " + s.name + " cannot start at login");
}

static void remove_service(const Service& s) {
    RegDeleteKeyValueW(HKEY_CURRENT_USER, run_key, config::from_utf8(s.run_name).wstring().c_str());
}

void start(const Service& s) {
    fs::create_directories(s.log.parent_path());
    std::string job = "Local\\" + s.unit + "-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    process::start_detached({ windows_argv(s), util::rant_home(), {} }, "NUL", job);
}

/* The program waits on its stop event and stops when it is set. Without one, or when it does
 * not stop in 5 s, the process is ended. */
static void end_process(const Service& s, uint64_t pid) {
    bool asked = false;
    if (HANDLE event = OpenEventW(EVENT_MODIFY_STATE, FALSE, config::from_utf8(s.stop_event).wstring().c_str())) {
        asked = SetEvent(event) != 0;
        CloseHandle(event);
    }
    HANDLE proc = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE, FALSE, (DWORD)pid);
    if (!proc) return;
    if (WaitForSingleObject(proc, asked ? 5000 : 0) != WAIT_OBJECT_0) {
        TerminateProcess(proc, 1);
        WaitForSingleObject(proc, 2000);
    }
    CloseHandle(proc);
}

#else

static fs::path service_file(const Service& s) {
#ifdef __APPLE__
    return util::home_dir() / "Library" / "LaunchAgents" / (s.label + ".plist");
#else
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = util::home_dir() / ".config";
    return config / "systemd" / "user" / (s.unit + ".service");
#endif
}

static std::string service_text(const Service& s) {
#ifdef __APPLE__
    return launchd_plist(s);
#else
    return systemd_unit(s);
#endif
}

/* Another home's program, such as the real one while a test runs, is not this one. */
bool enabled(const Service& s) {
    std::ifstream in(service_file(s));
    std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    return text.find(native(s.exe)) != std::string::npos;
}

#ifdef __APPLE__
static std::string agent(const Service& s) { return "gui/" + std::to_string(getuid()) + "/" + s.label; }
#endif

/* A service manager command. A failure is the user's to read, so its output stays on screen. */
static void manage(const std::vector<std::string>& argv, bool must = true) {
    if (process::run({ argv, fs::temp_directory_path(), {} }) != 0 && must)
        throw app::Failure("`" + process::shown(argv) + "` failed, see what it said above");
}

void define(const Service& s) {
    fs::path file = service_file(s);
    std::error_code ec;
    fs::create_directories(file.parent_path(), ec);
    std::ofstream(file, std::ios::binary) << service_text(s);
    if (!std::ifstream(file)) throw app::Failure("cannot write " + u8(file));
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent(s) }, false);
    manage({ "launchctl", "bootstrap", "gui/" + std::to_string(getuid()), native(file) });
#else
    manage({ "systemctl", "--user", "daemon-reload" });
    manage({ "systemctl", "--user", "enable", s.unit });
#endif
}

static void remove_service(const Service& s) {
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent(s) }, false);
#else
    manage({ "systemctl", "--user", "disable", "--now", s.unit }, false);
#endif
    std::error_code ec;
    fs::remove(service_file(s), ec);
#ifndef __APPLE__
    manage({ "systemctl", "--user", "daemon-reload" }, false);
#endif
}

static void require_enabled(const Service& s) {
    if (!enabled(s)) throw app::Failure(s.name + " is not enabled, run `rant " + s.command + " enable`");
}

void start(const Service& s) {
    require_enabled(s);
#ifdef __APPLE__
    manage({ "launchctl", "kickstart", agent(s) });
#else
    manage({ "systemctl", "--user", "start", s.unit });
#endif
}

#endif

bool stop(const Service& s) {
    std::optional<State> st = running(s);
    if (!st) return false;
#ifdef _WIN32
    end_process(s, st->pid);
#else
    if (enabled(s)) {
#ifdef __APPLE__
        manage({ "launchctl", "kill", "SIGTERM", agent(s) });
#else
        manage({ "systemctl", "--user", "stop", s.unit });
#endif
    } else {
        process::stop_pid(st->pid, std::chrono::seconds(5));
    }
#endif
    for (int i = 0; i < 50 && process::pid_alive(st->pid); i++) std::this_thread::sleep_for(std::chrono::milliseconds(100));
    if (process::pid_alive(st->pid)) throw app::Failure(s.name + " (process " + std::to_string(st->pid) + ") did not stop");
    return true;
}

void restart(const Service& s) {
#ifdef _WIN32
    stop(s);
    start(s);
#else
    require_enabled(s);
#ifdef __APPLE__
    manage({ "launchctl", "kickstart", "-k", agent(s) });
#else
    manage({ "systemctl", "--user", "restart", s.unit });
#endif
#endif
}

void disable(const Service& s) {
    if (enabled(s)) remove_service(s);
    stop(s);
}

State wait_listening(const Service& s, uint64_t not_pid) {
    for (int i = 0; i < 100; i++) {
        if (auto st = running(s); st && st->pid != not_pid) return *st;
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    throw app::Failure(s.name + " did not start listening in 10 s, see `rant " + s.command + " logs`");
}

std::string version_of(const fs::path& exe) {
    std::optional<std::string> text = process::output({ { native(exe), "--version" }, fs::temp_directory_path(), {} });
    if (!text) throw app::Failure(u8(exe) + " does not run here, `--version` failed");
    while (!text->empty() && std::isspace((unsigned char)text->back())) text->pop_back();
    return text->substr(text->find_last_of(" \t") == std::string::npos ? 0 : text->find_last_of(" \t") + 1);
}

void install_from(app::Context& ctx, const Service& s, const std::string& from) {
    fs::path src = ctx.cwd() / config::from_utf8(from);
    if (!fs::is_regular_file(src))
        throw app::Failure("no file `" + from + "`, give the path of a " + s.exe.stem().u8string() + " you built");
    std::string version = version_of(src);
    stop(s);    /* a running program cannot be overwritten on Windows */
    std::error_code ec;
    fs::create_directories(s.exe.parent_path(), ec);
    fs::copy_file(src, s.exe, fs::copy_options::overwrite_existing, ec);
    if (ec) throw app::Failure("cannot copy " + u8(src) + " to " + u8(s.exe) + ": " + ec.message());
    std::ofstream(s.version_file) << version << "\n";
    ctx.out.note("installed " + s.name + " " + version + " as " + u8(s.exe));
}

void note_linger(const app::Context& ctx) {
#if !defined(_WIN32) && !defined(__APPLE__)
    std::optional<std::string> linger = process::output({ { "loginctl", "show-user", std::to_string(getuid()), "-p", "Linger" }, fs::temp_directory_path(), {} });
    if (linger && linger->find("Linger=no") != std::string::npos)
        ctx.out.note("to start it at boot without a login, run `sudo loginctl enable-linger $USER`");
#else
    (void)ctx;
#endif
}

void require_installed(const Service& s) {
    if (!fs::is_regular_file(s.exe)) throw app::Failure(s.name + " is not installed, run `rant " + s.command + " enable`");
}

const std::vector<std::string>& subcommands() {
    static const std::vector<std::string> list = { "enable", "disable", "start", "stop", "restart", "status", "logs", "run" };
    return list;
}

/* "enable, disable, ... or run" */
static std::string listed() {
    std::string out;
    auto& subs = subcommands();
    for (size_t i = 0; i < subs.size(); i++) out += (i == 0 ? "" : i + 1 == subs.size() ? " or " : ", ") + subs[i];
    return out;
}

void check_usage(const app::Context& ctx, const Service& s, const std::vector<std::string>& w,
                 const std::vector<std::string>& enable_options) {
    if (w.empty()) throw app::UsageError(s.command + " needs a command: " + listed());
    const std::string& name = w[0];
    bool known = false;
    for (auto& sub : subcommands()) known = known || sub == name;
    if (!known) throw app::UsageError("unknown " + s.command + " command `" + name + "`, it takes " + listed());
    for (auto& o : enable_options)
        if (name != "enable" && ctx.args.has(o)) throw app::UsageError("--" + o + " goes with `rant " + s.command + " enable`");
    if (name != "logs" && ctx.args.has("follow")) throw app::UsageError("--follow goes with `rant " + s.command + " logs`");
    if (name != "run" && w.size() > 1) throw app::UsageError(s.command + " " + name + " takes no arguments");
}

static int disable_command(app::Context& ctx, const Service& s) {
    if (!enabled(s) && !running(s)) {
        ctx.out.note(s.name + " is not enabled");
        return 0;
    }
    disable(s);
    ctx.out.note("disabled " + s.name + ", " + s.stays);
    return 0;
}

static int start_command(app::Context& ctx, const Service& s) {
    require_installed(s);
    if (auto st = running(s)) {
        ctx.out.note(s.name + " already runs at " + st->url);
        return 0;
    }
    start(s);
    ctx.out.note("started " + s.name + " at " + wait_listening(s, 0).url);
    return 0;
}

static int stop_command(app::Context& ctx, const Service& s) {
    ctx.out.note(stop(s) ? "stopped " + s.name : s.name + " is not running");
    return 0;
}

static int restart_command(app::Context& ctx, const Service& s) {
    require_installed(s);
    uint64_t before = running(s) ? running(s)->pid : 0;
    restart(s);
    ctx.out.note("restarted " + s.name + " at " + wait_listening(s, before).url);
    return 0;
}

static int logs_command(app::Context& ctx, const Service& s) {
    bool follow = ctx.args.has("follow");
#if !defined(_WIN32) && !defined(__APPLE__)
    std::vector<std::string> argv = { "journalctl", "--user", "-u", s.unit, "-n", "100", "--no-pager" };
    if (follow) argv.push_back("-f");
    return process::run({ argv, ctx.cwd(), {} });
#else
    std::error_code ec;
    if (!fs::is_regular_file(s.log, ec)) throw app::Failure(s.name + " has no log yet, start it with `rant " + s.command + " start`");
    run::Follower follower(s.log);
    for (auto& l : run::tail(s.log, 100)) ctx.out.line(l);
    if (!follow) return 0;
    util::catch_interrupt();
    while (!util::interrupted()) {
        for (auto& l : follower.poll()) ctx.out.line(l);
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
    return 0;
#endif
}

/* The program in the foreground with its service arguments, then the words after `--`. */
static int run_command(app::Context& ctx, const Service& s, const std::vector<std::string>& words) {
    require_installed(s);
    std::vector<std::string> argv = { native(s.exe) };
    argv.insert(argv.end(), s.args.begin(), s.args.end());
    argv.insert(argv.end(), words.begin() + 1, words.end());
    util::catch_interrupt();
    return process::run({ argv, ctx.cwd(), {} });
}

std::optional<int> run_shared(app::Context& ctx, const Service& s, const std::vector<std::string>& words) {
    const std::string& sub = words[0];
    if (sub == "disable") return disable_command(ctx, s);
    if (sub == "start") return start_command(ctx, s);
    if (sub == "stop") return stop_command(ctx, s);
    if (sub == "restart") return restart_command(ctx, s);
    if (sub == "logs") return logs_command(ctx, s);
    if (sub == "run") return run_command(ctx, s, words);
    return std::nullopt;
}

}
