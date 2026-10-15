#include "check.hpp"
#include "commands/vscode.hpp"

namespace vscode = commands::vscode;

TEST(listing_gives_the_extension_version) {
    CHECK_EQ(vscode::version_in("ms-python.python@2026.1.0\nkosmosisdire.vscode-rant@0.2.1\n"), std::string("0.2.1"));
}

TEST(listing_matches_the_id_without_case_and_line_ends) {
    CHECK_EQ(vscode::version_in("KosmosisDire.vscode-rant@1.0.0\r\n"), std::string("1.0.0"));
}

TEST(listing_without_the_extension_is_empty) {
    CHECK_EQ(vscode::version_in(""), std::string());
    CHECK_EQ(vscode::version_in("kosmosisdire.vscode-rant-extra@1.0.0\nother.vscode-rant@1.0.0\n"), std::string());
}
