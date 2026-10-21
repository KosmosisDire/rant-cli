#include "services/entry.hpp"

#include <fstream>
#include <iterator>
#include <optional>

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
#include <sddl.h>
#else
#include <unistd.h>
#endif

namespace services::entry {

/* A path as a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

const char* word(When when) { return when == When::Boot ? "boot" : "login"; }

std::vector<std::string> argv(When when, const fs::path& rant, const fs::path& home) {
    return { native(rant), "__services", when == When::Boot ? "--boot" : "--login", native(home) };
}

static std::vector<std::string> mine(When when) { return argv(when, process::self_path(), util::rant_home()); }

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

static std::string exec_line(const std::vector<std::string>& argv) {
    std::string exec;
    for (auto& a : argv) exec += (exec.empty() ? "" : " ") + systemd_word(a);
    return exec;
}

/* Only the service process is stopped with the unit, so the nodes it started keep running. */
std::string systemd_user_unit(const std::vector<std::string>& argv) {
    return "[Unit]\n"
           "Description=Rant service process\n"
           "\n"
           "[Service]\n"
           "ExecStart=" + exec_line(argv) + "\n"
           "KillMode=process\n"
           "\n"
           "[Install]\n"
           "WantedBy=default.target\n";
}

std::string systemd_system_unit(const std::vector<std::string>& argv, const std::string& user) {
    return "[Unit]\n"
           "Description=Rant service process of " + user + "\n"
           "Wants=network-online.target\n"
           "After=network-online.target\n"
           "\n"
           "[Service]\n"
           "User=" + user + "\n"
           "ExecStart=" + exec_line(argv) + "\n"
           "KillMode=process\n"
           "\n"
           "[Install]\n"
           "WantedBy=multi-user.target\n";
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
std::string launchd_plist(const std::vector<std::string>& argv, const std::string& label, const std::string& user,
                          const fs::path& log) {
    auto str = [](const std::string& v) { return "<string>" + xml_text(v) + "</string>"; };
    std::string args;
    for (auto& a : argv) args += "    " + str(a) + "\n";
    return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
           "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
           "<plist version=\"1.0\">\n"
           "<dict>\n"
           "  <key>Label</key>\n"
           "  " + str(label) + "\n" +
           (user.empty() ? "" : "  <key>UserName</key>\n  " + str(user) + "\n") +
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

/* Runs as the user without a stored password, never stopped for time or battery, at normal
 * priority, and anyone signed in may read it, so rant checks it without admin. */
std::string task_xml(const std::vector<std::string>& argv, const std::string& user_sid) {
    std::string args = process::windows_command_line({ argv.begin() + 1, argv.end() });
    return "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n"
           "<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n"
           "  <RegistrationInfo>\n"
           "    <Description>Starts what rant enable named at boot</Description>\n"
           "    <SecurityDescriptor>D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;AU)</SecurityDescriptor>\n"
           "  </RegistrationInfo>\n"
           "  <Triggers>\n"
           "    <BootTrigger>\n"
           "      <Enabled>true</Enabled>\n"
           "    </BootTrigger>\n"
           "  </Triggers>\n"
           "  <Principals>\n"
           "    <Principal id=\"Author\">\n"
           "      <UserId>" + user_sid + "</UserId>\n"
           "      <LogonType>S4U</LogonType>\n"
           "      <RunLevel>LeastPrivilege</RunLevel>\n"
           "    </Principal>\n"
           "  </Principals>\n"
           "  <Settings>\n"
           "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n"
           "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n"
           "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n"
           "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n"
           "    <Priority>5</Priority>\n"
           "  </Settings>\n"
           "  <Actions Context=\"Author\">\n"
           "    <Exec>\n"
           "      <Command>" + xml_text(argv[0]) + "</Command>\n"
           "      <Arguments>" + xml_text(args) + "</Arguments>\n"
           "    </Exec>\n"
           "  </Actions>\n"
           "</Task>\n";
}

#ifdef _WIN32

static std::string utf8(const std::wstring& w) { return config::to_utf8(fs::path(w)); }

static std::string user_name() {
    wchar_t buf[257];
    DWORD n = 257;
    return GetUserNameW(buf, &n) ? utf8(buf) : "user";
}

static std::string user_sid() {
    HANDLE token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) throw app::Failure("cannot read who you are from Windows");
    DWORD size = 0;
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    std::vector<char> buf(size);
    std::string sid;
    wchar_t* text = nullptr;
    if (GetTokenInformation(token, TokenUser, buf.data(), size, &size) &&
        ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(buf.data())->User.Sid, &text)) {
        sid = utf8(text);
        LocalFree(text);
    }
    CloseHandle(token);
    if (sid.empty()) throw app::Failure("cannot read who you are from Windows");
    return sid;
}

static fs::path schtasks() { return util::env_path("SystemRoot") / "System32" / "schtasks.exe"; }

static std::string task_name() { return "Rant " + user_name(); }

std::string boot_place() { return "the scheduled task " + task_name(); }

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

static std::wstring login_line() { return config::from_utf8(process::windows_command_line(mine(When::Login))).wstring(); }

/* XML as schtasks prints it, its escapes undone. */
static std::string unescaped(std::string s) {
    for (auto [from, to] : { std::pair<const char*, const char*>{ "&quot;", "\"" }, { "&apos;", "'" }, { "&lt;", "<" }, { "&gt;", ">" }, { "&amp;", "&" } })
        for (size_t at = 0; (at = s.find(from, at)) != std::string::npos; at += std::char_traits<char>::length(to))
            s.replace(at, std::char_traits<char>::length(from), to);
    return s;
}

bool present(When when) {
    if (when == When::Login) return run_value() == login_line();
    std::optional<std::string> xml = process::output({ { config::to_utf8(schtasks()), "/query", "/tn", task_name(), "/xml", "ONE" }, fs::temp_directory_path(), {} });
    if (!xml) return false;
    std::vector<std::string> a = mine(When::Boot);
    std::string text = unescaped(*xml);
    return text.find("<Command>" + a[0] + "</Command>") != std::string::npos &&
           text.find("<Arguments>" + process::windows_command_line({ a.begin() + 1, a.end() }) + "</Arguments>") != std::string::npos;
}

void write(When when) {
    if (when == When::Login) {
        std::wstring v = login_line();
        if (RegSetKeyValueW(HKEY_CURRENT_USER, run_key, run_name, REG_SZ, v.c_str(), (DWORD)((v.size() + 1) * sizeof(wchar_t))) != ERROR_SUCCESS)
            throw app::Failure("cannot write the Run key of your user, so nothing can start at login");
        return;
    }
    if (present(When::Boot)) return;
    /* schtasks reads a task as UTF-16 */
    std::wstring text = config::from_utf8(task_xml(mine(When::Boot), user_sid())).wstring();
    fs::path file = fs::temp_directory_path() / ("rant-boot-" + std::to_string(GetCurrentProcessId()) + ".xml");
    {
        std::ofstream out(file, std::ios::binary);
        out.write("\xFF\xFE", 2);
        out.write(reinterpret_cast<const char*>(text.data()), (std::streamsize)(text.size() * sizeof(wchar_t)));
        if (!out) throw app::Failure("cannot write " + native(file));
    }
    int code = process::run_elevated({ { config::to_utf8(schtasks()), "/create", "/tn", task_name(), "/xml", native(file), "/f" }, fs::temp_directory_path(), {} });
    std::error_code ec;
    fs::remove(file, ec);
    if (code != 0 || !present(When::Boot)) throw app::Failure("Windows did not take " + boot_place() + ", so nothing starts at boot");
}

void remove(When when) {
    if (!present(when)) return;
    if (when == When::Login) {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, run_key, run_name);
        return;
    }
    process::run_elevated({ { config::to_utf8(schtasks()), "/delete", "/tn", task_name(), "/f" }, fs::temp_directory_path(), {} });
}

#else

static std::string user_name() {
    for (const char* name : { "USER", "LOGNAME" })
        if (const char* v = std::getenv(name); v && *v) return v;
    std::optional<std::string> id = process::output({ { "id", "-un" }, fs::temp_directory_path(), {} });
    std::string out = id ? *id : "";
    while (!out.empty() && (out.back() == '\n' || out.back() == '\r')) out.pop_back();
    if (out.empty()) throw app::Failure("cannot tell who you are, set USER");
    return out;
}

#ifdef __APPLE__
static std::string boot_label() { return "dev.rantlib.rant." + user_name(); }
static fs::path boot_file() { return fs::path("/Library/LaunchDaemons") / (boot_label() + ".plist"); }
static fs::path login_file() { return util::home_dir() / "Library" / "LaunchAgents" / "dev.rantlib.rant.plist"; }
std::string boot_place() { return "the launch daemon " + native(boot_file()); }
#else
static std::string boot_unit() { return "rant-" + user_name() + ".service"; }
static fs::path boot_file() { return fs::path("/etc/systemd/system") / boot_unit(); }
static fs::path login_file() {
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = util::home_dir() / ".config";
    return config / "systemd" / "user" / "rant.service";
}
std::string boot_place() { return "the system unit " + boot_unit(); }
#endif

static fs::path file(When when) { return when == When::Boot ? boot_file() : login_file(); }

static std::string text(When when) {
#ifdef __APPLE__
    return when == When::Boot ? launchd_plist(mine(when), boot_label(), user_name(), log_file())
                              : launchd_plist(mine(when), "dev.rantlib.rant", "", log_file());
#else
    return when == When::Boot ? systemd_system_unit(mine(when), user_name()) : systemd_user_unit(mine(when));
#endif
}

bool present(When when) {
    std::ifstream in(file(when), std::ios::binary);
    return in && std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()) == text(when);
}

/* A service manager command. A failure is the user's to read, so its output stays on screen. */
static void manage(const std::vector<std::string>& argv, bool must = true) {
    if (process::run({ argv, fs::temp_directory_path(), {} }) != 0 && must)
        throw app::Failure("`" + process::shown(argv) + "` failed, see what it said above");
}

#ifdef __APPLE__
static std::string agent() { return "gui/" + std::to_string(getuid()) + "/dev.rantlib.rant"; }
#endif

/* The boot entry goes in place as root in one sudo, so the password is asked once. */
static void write_boot() {
    fs::path tmp = fs::temp_directory_path() / ("rant-boot-" + std::to_string(getpid()));
    std::ofstream(tmp, std::ios::binary) << text(When::Boot);
#ifdef __APPLE__
    std::vector<std::string> argv = { "sh", "-c", "install -m 644 \"$1\" \"$2\"", "rant", native(tmp), native(boot_file()) };
#else
    std::vector<std::string> argv = { "sh", "-c", "install -m 644 \"$1\" \"$2\" && systemctl daemon-reload && systemctl enable \"$3\"",
                                      "rant", native(tmp), native(boot_file()), boot_unit() };
#endif
    int code = process::run_elevated({ argv, fs::temp_directory_path(), {} });
    std::error_code ec;
    fs::remove(tmp, ec);
    if (code != 0 || !present(When::Boot)) throw app::Failure("cannot add " + boot_place() + ", so nothing starts at boot");
}

void write(When when) {
    if (present(when)) return;
    if (when == When::Boot) return write_boot();
    fs::path f = login_file();
    std::error_code ec;
    fs::create_directories(f.parent_path(), ec);
    std::ofstream(f, std::ios::binary) << text(When::Login);
    if (!present(When::Login)) throw app::Failure("cannot write " + native(f) + ", so nothing can start at login");
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
    manage({ "launchctl", "bootstrap", "gui/" + std::to_string(getuid()), native(f) });
#else
    manage({ "systemctl", "--user", "daemon-reload" });
    manage({ "systemctl", "--user", "enable", "rant.service" });
#endif
}

void remove(When when) {
    if (!present(when)) return;
    if (when == When::Boot) {
#ifdef __APPLE__
        std::vector<std::string> argv = { "sh", "-c", "launchctl bootout system/\"$1\"; rm -f \"$2\"", "rant", boot_label(), native(boot_file()) };
#else
        std::vector<std::string> argv = { "sh", "-c", "systemctl disable \"$1\"; rm -f \"$2\"; systemctl daemon-reload", "rant", boot_unit(), native(boot_file()) };
#endif
        process::run_elevated({ argv, fs::temp_directory_path(), {} });
        return;
    }
#ifdef __APPLE__
    manage({ "launchctl", "bootout", agent() }, false);
#else
    manage({ "systemctl", "--user", "disable", "rant.service" }, false);
#endif
    std::error_code ec;
    fs::remove(login_file(), ec);
#ifndef __APPLE__
    manage({ "systemctl", "--user", "daemon-reload" }, false);
#endif
}

#endif

}
