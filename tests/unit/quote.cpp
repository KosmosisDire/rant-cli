#include <stdexcept>

#include "check.hpp"
#include "process/quote.hpp"

using process::batch_command_line;
using process::windows_command_line;

TEST(plain_words_stay_bare) {
    CHECK_EQ(windows_command_line({ "C:/bin/a.exe", "x", "--y=1" }), std::string("C:/bin/a.exe x --y=1"));
}

TEST(spaces_and_empty_words_are_quoted) {
    CHECK_EQ(windows_command_line({ "a", "b c", "" }), std::string("a \"b c\" \"\""));
}

TEST(quotes_and_backslashes_follow_the_msvc_rules) {
    CHECK_EQ(windows_command_line({ "a", "say \"hi\"" }), std::string("a \"say \\\"hi\\\"\""));
    CHECK_EQ(windows_command_line({ "a", "C:\\dir with space\\" }), std::string("a \"C:\\dir with space\\\\\""));
    CHECK_EQ(windows_command_line({ "a", "x\\\\\"y" }), std::string("a x\\\\\\\\\\\"y"));
    CHECK_EQ(windows_command_line({ "a", "C:\\plain\\" }), std::string("a C:\\plain\\"));
}

TEST(batch_arguments_never_reach_cmd_as_code) {
    std::string cmd = batch_command_line("C:\\t\\run.cmd", { "&calc", "%PATH%", "a\"b", "ok" });
    CHECK_EQ(cmd, std::string("cmd.exe /e:ON /v:OFF /d /c \"\"C:\\t\\run.cmd\" \"&calc\" \"%%cd:~,%PATH%%cd:~,%\" \"a\"\"b\" ok\""));
}

TEST(batch_line_breaks_are_refused) {
    CHECK_THROWS(batch_command_line("C:\\t\\run.cmd", { "a\nb" }));
    CHECK_THROWS(batch_command_line("C:\\t\\run.cmd", { "a\rb" }));
    CHECK_THROWS(batch_command_line("C:\\t\\bad\".cmd", {}));
}

TEST(batch_trailing_backslash_is_quoted) {
    CHECK_EQ(batch_command_line("r.bat", { "dir\\" }), std::string("cmd.exe /e:ON /v:OFF /d /c \"\"r.bat\" \"dir\\\\\"\""));
}
