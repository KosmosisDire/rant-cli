#include "check.hpp"
#include "commands/bridge.hpp"
#include "commands/service.hpp"
#include "process/quote.hpp"

namespace service = commands::service;

/* A service shaped as the bridge, at the paths given. */
static service::Service bridge_at(const std::string& exe, const std::string& config, const std::string& log) {
    service::Service s = commands::bridge::service();
    s.exe = exe;
    s.args = { "--config", config };
    s.windows_args = { "--supervise", "--log-file", log };
    s.log = log;
    return s;
}

TEST(state_lines_are_read) {
    auto s = service::parse_state("pid: 4242\nurl: ws://127.0.0.1:7480\nversion: 0.1.0\nstarted: 1760000000\n");
    CHECK(s.has_value());
    CHECK_EQ(s->pid, 4242u);
    CHECK_EQ(s->url, std::string("ws://127.0.0.1:7480"));
    CHECK_EQ(s->version, std::string("0.1.0"));
    CHECK_EQ(s->started, 1760000000ll);
}

TEST(state_reading_tolerates_line_ends_and_unknown_lines) {
    auto s = service::parse_state("note: hello\r\npid:7\r\nurl:   ws://[::1]:7480  \r\n\r\nno colon here\n");
    CHECK(s.has_value());
    CHECK_EQ(s->pid, 7u);
    CHECK_EQ(s->url, std::string("ws://[::1]:7480"));
    CHECK_EQ(s->started, 0ll);
}

TEST(state_without_a_pid_is_none) {
    CHECK(!service::parse_state("").has_value());
    CHECK(!service::parse_state("url: ws://127.0.0.1:7480\n").has_value());
    CHECK(!service::parse_state("pid: soon\n").has_value());
}

TEST(windows_start_runs_the_supervisor_with_a_log) {
    std::vector<std::string> argv =
        service::windows_argv(bridge_at("C:\\Users\\a b\\.rant\\bin\\rant-bridge.exe", "", "C:\\Users\\a b\\.rant\\logs\\bridge.log"));
    CHECK_EQ(process::windows_command_line(argv),
             std::string("\"C:\\Users\\a b\\.rant\\bin\\rant-bridge.exe\" --supervise --log-file \"C:\\Users\\a b\\.rant\\logs\\bridge.log\""));
    CHECK_EQ(process::windows_command_line(service::windows_argv(bridge_at("C:\\r\\rant-bridge.exe", "", "C:\\r\\bridge.log"))),
             std::string("C:\\r\\rant-bridge.exe --supervise --log-file C:\\r\\bridge.log"));
}

TEST(systemd_unit_restarts_on_failure) {
    CHECK_EQ(service::systemd_unit(bridge_at("/home/a/.rant/bin/rant-bridge", "/home/a/.rant/bridge.yaml", "")),
             std::string("[Unit]\n"
                         "Description=Rant bridge\n"
                         "\n"
                         "[Service]\n"
                         "ExecStart=\"/home/a/.rant/bin/rant-bridge\" \"--config\" \"/home/a/.rant/bridge.yaml\"\n"
                         "Restart=on-failure\n"
                         "RestartSec=1\n"
                         "\n"
                         "[Install]\n"
                         "WantedBy=default.target\n"));
}

TEST(systemd_unit_escapes_what_systemd_expands) {
    std::string unit = service::systemd_unit(bridge_at("/home/a b/50%/rant-bridge", "/home/$x/bridge.yaml", ""));
    CHECK(unit.find("ExecStart=\"/home/a b/50%%/rant-bridge\" \"--config\" \"/home/$$x/bridge.yaml\"\n") != std::string::npos);
}

TEST(launchd_plist_keeps_the_agent_alive_after_a_failure) {
    CHECK_EQ(service::launchd_plist(bridge_at("/Users/a/.rant/bin/rant-bridge", "/Users/a/.rant/bridge.yaml", "/Users/a/.rant/logs/bridge.log")),
             std::string("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                         "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
                         "<plist version=\"1.0\">\n"
                         "<dict>\n"
                         "  <key>Label</key>\n"
                         "  <string>dev.rantlib.bridge</string>\n"
                         "  <key>ProgramArguments</key>\n"
                         "  <array>\n"
                         "    <string>/Users/a/.rant/bin/rant-bridge</string>\n"
                         "    <string>--config</string>\n"
                         "    <string>/Users/a/.rant/bridge.yaml</string>\n"
                         "  </array>\n"
                         "  <key>RunAtLoad</key>\n"
                         "  <true/>\n"
                         "  <key>KeepAlive</key>\n"
                         "  <dict>\n"
                         "    <key>SuccessfulExit</key>\n"
                         "    <false/>\n"
                         "  </dict>\n"
                         "  <key>StandardOutPath</key>\n"
                         "  <string>/Users/a/.rant/logs/bridge.log</string>\n"
                         "  <key>StandardErrorPath</key>\n"
                         "  <string>/Users/a/.rant/logs/bridge.log</string>\n"
                         "</dict>\n"
                         "</plist>\n"));
}

TEST(launchd_plist_escapes_xml) {
    std::string plist = service::launchd_plist(bridge_at("/Users/a&b/rant-bridge", "/c.yaml", "/l.log"));
    CHECK(plist.find("<string>/Users/a&amp;b/rant-bridge</string>") != std::string::npos);
}

/* Only reads: a program not installed never counts as enabled, whatever the real login entries hold. */
TEST(a_program_not_installed_is_never_enabled) {
    service::Service s = bridge_at("", "bridge.yaml", "bridge.log");
    s.run_name = "RantNoSuchService";
    s.unit = "rant-no-such-service";
    s.label = "dev.rantlib.no-such-service";
    CHECK(!service::enabled(s));
}
