#include "check.hpp"
#include "process/quote.hpp"
#include "services/entry.hpp"

namespace entry = services::entry;

using entry::When;

TEST(the_windows_login_entry_names_the_rant_and_its_home) {
    CHECK_EQ(process::windows_command_line(entry::argv(When::Login, "C:\\Users\\a b\\.rant\\bin\\rant.exe", "C:\\Users\\a b\\.rant")),
             std::string("\"C:\\Users\\a b\\.rant\\bin\\rant.exe\" __services --login \"C:\\Users\\a b\\.rant\""));
}

TEST(the_windows_boot_task_runs_as_the_user_at_boot_without_a_time_limit) {
    std::string xml = entry::task_xml(entry::argv(When::Boot, "C:\\r&d\\rant.exe", "C:\\Users\\a b\\.rant"), "S-1-5-21-1-2-3-1001");
    for (const char* part : { "<BootTrigger>", "<UserId>S-1-5-21-1-2-3-1001</UserId>", "<LogonType>S4U</LogonType>",
                              "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>", "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
                              "<Command>C:\\r&amp;d\\rant.exe</Command>",
                              "<Arguments>__services --boot \"C:\\Users\\a b\\.rant\"</Arguments>", "(A;;FR;;;AU)" })
        CHECK(xml.find(part) != std::string::npos);
}

TEST(the_systemd_boot_unit_runs_as_the_user_after_the_network) {
    CHECK_EQ(entry::systemd_system_unit(entry::argv(When::Boot, "/home/a/.rant/bin/rant", "/home/a/.rant"), "a"),
             std::string("[Unit]\n"
                         "Description=Rant service process of a\n"
                         "Wants=network-online.target\n"
                         "After=network-online.target\n"
                         "\n"
                         "[Service]\n"
                         "User=a\n"
                         "ExecStart=\"/home/a/.rant/bin/rant\" \"__services\" \"--boot\" \"/home/a/.rant\"\n"
                         "KillMode=process\n"
                         "\n"
                         "[Install]\n"
                         "WantedBy=multi-user.target\n"));
}

TEST(the_launchd_boot_daemon_runs_as_the_user) {
    std::string plist = entry::launchd_plist(entry::argv(When::Boot, "/Users/a/rant", "/Users/a/.rant"), "dev.rantlib.rant.a", "a", "/l.log");
    CHECK(plist.find("<key>UserName</key>\n  <string>a</string>\n") != std::string::npos);
    CHECK(plist.find("<string>--boot</string>") != std::string::npos);
}

TEST(the_systemd_login_unit_stops_only_the_service_process) {
    CHECK_EQ(entry::systemd_user_unit(entry::argv(When::Login, "/home/a/.rant/bin/rant", "/home/a/.rant")),
             std::string("[Unit]\n"
                         "Description=Rant service process\n"
                         "\n"
                         "[Service]\n"
                         "ExecStart=\"/home/a/.rant/bin/rant\" \"__services\" \"--login\" \"/home/a/.rant\"\n"
                         "KillMode=process\n"
                         "\n"
                         "[Install]\n"
                         "WantedBy=default.target\n"));
}

TEST(the_systemd_unit_escapes_what_systemd_expands) {
    std::string unit = entry::systemd_user_unit(entry::argv(When::Login, "/home/a b/50%/rant", "/home/$x/.rant"));
    CHECK(unit.find("ExecStart=\"/home/a b/50%%/rant\" \"__services\" \"--login\" \"/home/$$x/.rant\"\n") != std::string::npos);
}

TEST(the_launchd_agent_starts_once_and_leaves_the_nodes) {
    CHECK_EQ(entry::launchd_plist(entry::argv(When::Login, "/Users/a/.rant/bin/rant", "/Users/a/.rant"), "dev.rantlib.rant", "",
                                  "/Users/a/.rant/logs/services.log"),
             std::string("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                         "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
                         "<plist version=\"1.0\">\n"
                         "<dict>\n"
                         "  <key>Label</key>\n"
                         "  <string>dev.rantlib.rant</string>\n"
                         "  <key>ProgramArguments</key>\n"
                         "  <array>\n"
                         "    <string>/Users/a/.rant/bin/rant</string>\n"
                         "    <string>__services</string>\n"
                         "    <string>--login</string>\n"
                         "    <string>/Users/a/.rant</string>\n"
                         "  </array>\n"
                         "  <key>RunAtLoad</key>\n"
                         "  <true/>\n"
                         "  <key>AbandonProcessGroup</key>\n"
                         "  <true/>\n"
                         "  <key>StandardOutPath</key>\n"
                         "  <string>/Users/a/.rant/logs/services.log</string>\n"
                         "  <key>StandardErrorPath</key>\n"
                         "  <string>/Users/a/.rant/logs/services.log</string>\n"
                         "</dict>\n"
                         "</plist>\n"));
}

TEST(the_launchd_agent_escapes_xml) {
    std::string plist = entry::launchd_plist(entry::argv(When::Login, "/Users/a&b/rant", "/h"), "dev.rantlib.rant", "", "/l.log");
    CHECK(plist.find("<string>/Users/a&amp;b/rant</string>") != std::string::npos);
}

/* Only reads: the tests run another rant with another home, so no real entry names them. */
TEST(no_real_entry_names_a_test) {
    CHECK(!entry::present(When::Login));
    CHECK(!entry::present(When::Boot));
}
