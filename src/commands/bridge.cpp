#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>
#include <thread>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/bridge.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "complete/complete.hpp"
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

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static const char* bridge_repo = "KosmosisDire/rant-bridge";

#ifdef _WIN32
static const char* exe_name = "rant-bridge.exe";
#else
static const char* exe_name = "rant-bridge";
#endif

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

/* A path as a program or a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

namespace bridge {

fs::path installed() { return util::rant_home() / "bin" / exe_name; }
static fs::path version_file() { return util::rant_home() / "bin" / "rant-bridge.version"; }
static fs::path state_file() { return util::rant_home() / "bridge.state"; }
fs::path log_file() { return util::rant_home() / "logs" / "bridge.log"; }
fs::path config_path() { return util::rant_home() / "bridge.yaml"; }

std::string installed_version() {
    std::ifstream in(version_file());
    std::string v;
    std::getline(in, v);
    return v;
}

void install(const app::Context& ctx, const net::Release& r) {
    std::string asset = "rant-bridge-" + r.version + "-" + net::platform();
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("the bridge " + r.version + " release has no " + asset);
    ctx.out.note("downloading " + asset);
    net::download(a->url, installed(), true);
    std::ofstream(version_file()) << r.version << "\n";
    ctx.out.note("installed the bridge " + r.version + " as " + u8(installed()));
}

net::Release latest() { return net::release(bridge_repo); }

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

std::optional<State> running() {
    std::ifstream in(state_file());
    std::optional<State> s = parse_state(std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()));
    if (s && process::pid_alive(s->pid)) return s;
    return std::nullopt;
}

std::vector<std::string> windows_argv(const std::string& exe, const std::string& log) {
    return { exe, "--supervise", "--log-file", log };
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

std::string systemd_unit(const std::string& exe, const std::string& config) {
    return "[Unit]\n"
           "Description=Rant bridge\n"
           "\n"
           "[Service]\n"
           "ExecStart=" + systemd_word(exe) + " --config " + systemd_word(config) + "\n"
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

std::string launchd_plist(const std::string& exe, const std::string& config, const std::string& log) {
    auto s = [](const std::string& v) { return "<string>" + xml_text(v) + "</string>"; };
    return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
           "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
           "<plist version=\"1.0\">\n"
           "<dict>\n"
           "  <key>Label</key>\n"
           "  <string>dev.rantlib.bridge</string>\n"
           "  <key>ProgramArguments</key>\n"
           "  <array>\n"
           "    " + s(exe) + "\n"
           "    <string>--config</string>\n"
           "    " + s(config) + "\n"
           "  </array>\n"
           "  <key>RunAtLoad</key>\n"
           "  <true/>\n"
           "  <key>KeepAlive</key>\n"
           "  <dict>\n"
           "    <key>SuccessfulExit</key>\n"
           "    <false/>\n"
           "  </dict>\n"
           "  <key>StandardOutPath</key>\n"
           "  " + s(log) + "\n"
           "  <key>StandardErrorPath</key>\n"
           "  " + s(log) + "\n"
           "</dict>\n"
           "</plist>\n";
}

#ifdef _WIN32

static const wchar_t* run_key = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
static const wchar_t* run_name = L"RantBridge";

static std::wstring run_value() {
    DWORD size = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, run_name, RRF_RT_REG_SZ, nullptr, nullptr, &size) != ERROR_SUCCESS) return {};
    std::wstring v(size / sizeof(wchar_t), L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, run_key, run_name, RRF_RT_REG_SZ, nullptr, v.data(), &size) != ERROR_SUCCESS) return {};
    v.resize(wcslen(v.c_str()));
    return v;
}

static std::string command_line() { return process::windows_command_line(windows_argv(native(installed()), native(log_file()))); }

/* Another home's bridge, such as the real one while a test runs, is not this one. */
bool enabled() { return run_value().find(installed().wstring()) != std::wstring::npos; }

void define() {
    std::wstring v = config::from_utf8(command_line()).wstring();
    if (RegSetKeyValueW(HKEY_CURRENT_USER, run_key, run_name, REG_SZ, v.c_str(), (DWORD)((v.size() + 1) * sizeof(wchar_t))) != ERROR_SUCCESS)
        throw app::Failure("cannot write the Run key of your user, the bridge cannot start at login");
}

static void remove_service() { RegDeleteKeyValueW(HKEY_CURRENT_USER, run_key, run_name); }

void start() {
    fs::create_directories(log_file().parent_path());
    std::string job = "Local\\rant-bridge-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    process::start_detached({ windows_argv(native(installed()), native(log_file())), util::rant_home(), {} }, "NUL", job);
}

/* The bridge waits on this event and stops when it is set. Without one, or when it does not
 * stop in 5 s, the process is ended. */
static void end_process(uint64_t pid) {
    bool asked = false;
    if (HANDLE event = OpenEventW(EVENT_MODIFY_STATE, FALSE, L"Local\\rant-bridge-stop")) {
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

static fs::path service_file() {
#ifdef __APPLE__
    return util::home_dir() / "Library" / "LaunchAgents" / "dev.rantlib.bridge.plist";
#else
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = util::home_dir() / ".config";
    return config / "systemd" / "user" / "rant-bridge.service";
#endif
}

static std::string service_text() {
#ifdef __APPLE__
    return launchd_plist(native(installed()), native(config_path()), native(log_file()));
#else
    return systemd_unit(native(installed()), native(config_path()));
#endif
}

/* Another home's bridge, such as the real one while a test runs, is not this one. */
bool enabled() {
    std::ifstream in(service_file());
    std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    return text.find(native(installed())) != std::string::npos;
}

#ifdef __APPLE__
static std::string agent() { return "gui/" + std::to_string(getuid()) + "/dev.rantlib.bridge"; }
#endif

/* A service manager command. A failure is the user's to read, so its output stays on screen. */
static void manage(const std::vector<std::string>& argv, bool must = true) {
    if (process::run({ argv, fs::temp_directory_path(), {} }) != 0 && must)
        throw app::Failure("`" + process::shown(argv) + "` failed, see what it said above");
}

void define() {
    fs::path file = service_file();
    std::error_code ec;
    fs::create_directories(file.parent_path(), ec);
    std::ofstream(file, std::ios::binary) << service_text();
    if (!std::ifstream(file)) throw app::Failure("cannot write " + u8(file));
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
    manage({ "launchctl", "bootstrap", "gui/" + std::to_string(getuid()), native(file) });
#else
    manage({ "systemctl", "--user", "daemon-reload" });
    manage({ "systemctl", "--user", "enable", "rant-bridge" });
#endif
}

static void remove_service() {
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
#else
    manage({ "systemctl", "--user", "disable", "--now", "rant-bridge" }, false);
#endif
    std::error_code ec;
    fs::remove(service_file(), ec);
#ifndef __APPLE__
    manage({ "systemctl", "--user", "daemon-reload" }, false);
#endif
}

static void require_enabled() {
    if (!enabled()) throw app::Failure("the bridge is not enabled, run `rant bridge enable`");
}

void start() {
    require_enabled();
#ifdef __APPLE__
    manage({ "launchctl", "kickstart", agent() });
#else
    manage({ "systemctl", "--user", "start", "rant-bridge" });
#endif
}

#endif

bool stop() {
    std::optional<State> s = running();
    if (!s) return false;
#ifdef _WIN32
    end_process(s->pid);
#else
    if (enabled()) {
#ifdef __APPLE__
        manage({ "launchctl", "kill", "SIGTERM", agent() });
#else
        manage({ "systemctl", "--user", "stop", "rant-bridge" });
#endif
    } else {
        process::stop_pid(s->pid, std::chrono::seconds(5));
    }
#endif
    for (int i = 0; i < 50 && process::pid_alive(s->pid); i++) std::this_thread::sleep_for(std::chrono::milliseconds(100));
    if (process::pid_alive(s->pid)) throw app::Failure("the bridge (process " + std::to_string(s->pid) + ") did not stop");
    return true;
}

void restart() {
#ifdef _WIN32
    stop();
    start();
#else
    require_enabled();
#ifdef __APPLE__
    manage({ "launchctl", "kickstart", "-k", agent() });
#else
    manage({ "systemctl", "--user", "restart", "rant-bridge" });
#endif
#endif
}

void disable() {
    if (enabled()) remove_service();
    stop();
}

}

/* The state of a bridge listening under another process id than not_pid, once it writes
 * one, or a failure. A restart passes the old id so the state it replaces is not taken. */
static bridge::State wait_listening(uint64_t not_pid) {
    for (int i = 0; i < 100; i++) {
        if (auto s = bridge::running(); s && s->pid != not_pid) return *s;
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    throw app::Failure("the bridge did not start listening in 10 s, see `rant bridge logs`");
}

static void require_installed() {
    if (!fs::is_regular_file(bridge::installed())) throw app::Failure("the bridge is not installed, run `rant bridge enable`");
}

/* What `exe --version` prints after the program name. */
static std::string version_of(const fs::path& exe) {
    std::optional<std::string> text = process::output({ { native(exe), "--version" }, fs::temp_directory_path(), {} });
    if (!text) throw app::Failure(u8(exe) + " does not run here, `--version` failed");
    while (!text->empty() && std::isspace((unsigned char)text->back())) text->pop_back();
    return text->substr(text->find_last_of(" \t") == std::string::npos ? 0 : text->find_last_of(" \t") + 1);
}

/* The bridge binary in place: the file given, else the one installed, else the newest release. */
static void ensure_binary(app::Context& ctx) {
    fs::path exe = bridge::installed();
    if (auto from = ctx.args.get("from")) {
        fs::path src = ctx.cwd() / config::from_utf8(*from);
        if (!fs::is_regular_file(src)) throw app::Failure("no file `" + *from + "`, give the path of a rant-bridge you built");
        std::string version = version_of(src);
        bridge::stop();    /* a running program cannot be overwritten on Windows */
        std::error_code ec;
        fs::create_directories(exe.parent_path(), ec);
        fs::copy_file(src, exe, fs::copy_options::overwrite_existing, ec);
        if (ec) throw app::Failure("cannot copy " + u8(src) + " to " + u8(exe) + ": " + ec.message());
        std::ofstream(exe.parent_path() / "rant-bridge.version") << version << "\n";
        ctx.out.note("installed the bridge " + version + " as " + u8(exe));
    } else if (!fs::is_regular_file(exe)) {
        net::Release r;
        try {
            r = bridge::latest();
        } catch (const app::Failure& e) {
            throw app::Failure(std::string(e.what()) + ". To use a bridge you built, run `rant bridge enable --from PATH`");
        }
        bridge::install(ctx, r);
    }
}

static int bridge_enable(app::Context& ctx) {
    ensure_binary(ctx);
    fs::path exe = bridge::installed();
    std::error_code ec;
    fs::create_directories(util::rant_home() / "logs", ec);
    if (!process::output({ { native(exe), "--init-config", "--config", native(bridge::config_path()) }, util::rant_home(), {} }))
        throw app::Failure("`rant-bridge --init-config` failed, run it by hand to see why");

    bridge::define();
    if (!bridge::running()) bridge::start();
    bridge::State s = wait_listening(0);
    ctx.out.note("enabled the bridge, it starts when you log in");
#if !defined(_WIN32) && !defined(__APPLE__)
    std::optional<std::string> linger = process::output({ { "loginctl", "show-user", std::to_string(getuid()), "-p", "Linger" }, fs::temp_directory_path(), {} });
    if (linger && linger->find("Linger=no") != std::string::npos)
        ctx.out.note("to start it at boot without a login, run `sudo loginctl enable-linger $USER`");
#endif
    print_value(ctx, json{ { "url", s.url }, { "config", u8(bridge::config_path()) } });
    return 0;
}

static int bridge_disable(app::Context& ctx) {
    if (!bridge::enabled() && !bridge::running()) {
        ctx.out.note("the bridge is not enabled");
        return 0;
    }
    bridge::disable();
    ctx.out.note("disabled the bridge, its binary and " + u8(bridge::config_path()) + " stay");
    return 0;
}

static int bridge_start(app::Context& ctx) {
    require_installed();
    if (auto s = bridge::running()) {
        ctx.out.note("the bridge already runs at " + s->url);
        return 0;
    }
    bridge::start();
    ctx.out.note("started the bridge at " + wait_listening(0).url);
    return 0;
}

static int bridge_stop(app::Context& ctx) {
    ctx.out.note(bridge::stop() ? "stopped the bridge" : "the bridge is not running");
    return 0;
}

static int bridge_restart(app::Context& ctx) {
    require_installed();
    uint64_t before = bridge::running() ? bridge::running()->pid : 0;
    bridge::restart();
    ctx.out.note("restarted the bridge at " + wait_listening(before).url);
    return 0;
}

static int bridge_status(app::Context& ctx) {
    std::optional<bridge::State> s = bridge::running();
    std::string version = bridge::installed_version();
    bool installed = fs::is_regular_file(bridge::installed());
    json j = { { "installed", installed },
               { "binary", u8(bridge::installed()) },
               { "version", version.empty() ? json(nullptr) : json(version) },
               { "enabled", bridge::enabled() },
               { "running", s.has_value() },
               { "url", s ? json(s->url) : json(nullptr) },
               { "pid", s ? json(s->pid) : json(nullptr) },
               { "config", u8(bridge::config_path()) } };
    print_value(ctx, j);
    if (!installed && !ctx.json) ctx.out.note("the bridge is not installed, run `rant bridge enable`");
    return 0;
}

static int bridge_logs(app::Context& ctx) {
    bool follow = ctx.args.has("follow");
#if !defined(_WIN32) && !defined(__APPLE__)
    std::vector<std::string> argv = { "journalctl", "--user", "-u", "rant-bridge", "-n", "100", "--no-pager" };
    if (follow) argv.push_back("-f");
    return process::run({ argv, ctx.cwd(), {} });
#else
    fs::path log = bridge::log_file();
    std::error_code ec;
    if (!fs::is_regular_file(log, ec)) throw app::Failure("the bridge has no log yet, start it with `rant bridge start`");
    run::Follower follower(log);
    for (auto& l : run::tail(log, 100)) ctx.out.line(l);
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

/* The bridge in the foreground, the words after `--` as its own arguments. */
static int bridge_run(app::Context& ctx) {
    require_installed();
    std::vector<std::string> argv = { native(bridge::installed()) };
    argv.insert(argv.end(), ctx.args.words.begin() + 1, ctx.args.words.end());
    util::catch_interrupt();
    return process::run({ argv, ctx.cwd(), {} });
}

struct Sub {
    const char* name;
    int       (*run)(app::Context&);
};

static const std::vector<Sub>& subs() {
    static const std::vector<Sub> list = { { "enable", bridge_enable }, { "disable", bridge_disable }, { "start", bridge_start },
                                           { "stop", bridge_stop }, { "restart", bridge_restart }, { "status", bridge_status },
                                           { "logs", bridge_logs }, { "run", bridge_run } };
    return list;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) throw app::UsageError("bridge needs a command, see `rant bridge --help`");
    const std::string& name = w[0];
    for (auto& s : subs()) {
        if (name != s.name) continue;
        if (name != "enable" && ctx.args.has("from")) throw app::UsageError("--from goes with `rant bridge enable`");
        if (name != "logs" && ctx.args.has("follow")) throw app::UsageError("--follow goes with `rant bridge logs`");
        if (name != "run" && w.size() > 1) throw app::UsageError("bridge " + name + " takes no arguments");
        return s.run(ctx);
    }
    throw app::UsageError("unknown bridge command `" + name + "`, see `rant bridge --help`");
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    std::vector<std::string> out;
    for (auto& s : subs()) out.emplace_back(s.name);
    return { out };
}

app::Command bridge_() {
    app::Command c{ "bridge", "enable | disable | start | stop | restart | status | logs | run [-- args...]",
                    "run the bridge, a background server that connects web frontends to the mesh, config in ~/.rant/bridge.yaml",
                    app::Section::Setup,
                    { { "from", 0, "PATH", "with enable: install this rant-bridge instead of downloading the newest release" },
                      { "follow", 'f', "", "with logs: keep printing new lines until Ctrl-C" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
