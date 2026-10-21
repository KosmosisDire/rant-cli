#include "check.hpp"
#include "process/quote.hpp"
#include "services/entry.hpp"

namespace entry = services::entry;

TEST(the_windows_entry_names_the_rant_and_its_home) {
    CHECK_EQ(process::windows_command_line(entry::argv("C:\\Users\\a b\\.rant\\bin\\rant.exe", "C:\\Users\\a b\\.rant")),
             std::string("\"C:\\Users\\a b\\.rant\\bin\\rant.exe\" __services --login \"C:\\Users\\a b\\.rant\""));
}

TEST(the_systemd_unit_stops_only_the_service_process) {
    CHECK_EQ(entry::systemd_unit(entry::argv("/home/a/.rant/bin/rant", "/home/a/.rant")),
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
    std::string unit = entry::systemd_unit(entry::argv("/home/a b/50%/rant", "/home/$x/.rant"));
    CHECK(unit.find("ExecStart=\"/home/a b/50%%/rant\" \"__services\" \"--login\" \"/home/$$x/.rant\"\n") != std::string::npos);
}

TEST(the_launchd_agent_starts_once_and_leaves_the_nodes) {
    CHECK_EQ(entry::launchd_plist(entry::argv("/Users/a/.rant/bin/rant", "/Users/a/.rant"), "/Users/a/.rant/logs/services.log"),
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
    std::string plist = entry::launchd_plist(entry::argv("/Users/a&b/rant", "/h"), "/l.log");
    CHECK(plist.find("<string>/Users/a&amp;b/rant</string>") != std::string::npos);
}

/* Only reads: the tests run another rant with another home, so no real entry names them. */
TEST(no_real_entry_names_a_test) {
    CHECK(!entry::present());
}
