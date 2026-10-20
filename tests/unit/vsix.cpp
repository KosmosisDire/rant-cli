#include "check.hpp"
#include "packages/vsix.hpp"

namespace fs = std::filesystem;
namespace vsix = packages::vsix;

static const std::string id = "kosmosisdire.vscode-rant";

using Ids = std::vector<std::string>;

#ifdef _WIN32
static const std::string root_uri = "file:///c%3A/Main/RANT";
static const fs::path root = "C:\\Main\\RANT";
#else
static const std::string root_uri = "file:///home/me/RANT";
static const fs::path root = "/home/me/RANT";
#endif

/* A storage.json as VS Code writes it: the built in Agents profile shares the default's extensions. */
static const std::string storage = R"({
  "userDataProfiles": [
    { "location": "builtin/agents", "name": "Agents", "useDefaultFlags": { "extensions": true, "settings": true } },
    { "location": "-500e13d6", "name": "Mixed" },
    { "location": "-3b410830", "name": "Rust", "icon": "robot" }
  ],
  "profileAssociations": { "workspaces": {
    ")" + root_uri + R"(": "-500e13d6",
    ")" + root_uri + R"(/cli-rant": "-3b410830",
    ")" + root_uri + R"(%20Old": "__default__profile__",
    "vscode-remote://ssh-remote%2Bhost/home/me/RANT": "-3b410830"
  } }
})";

TEST(listing_gives_the_extension_version) {
    CHECK_EQ(vsix::version_in("ms-python.python@2026.1.0\nkosmosisdire.vscode-rant@0.2.1\n", id), std::string("0.2.1"));
}

TEST(listing_matches_the_id_without_case_and_line_ends) {
    CHECK_EQ(vsix::version_in("KosmosisDire.vscode-rant@1.0.0\r\n", id), std::string("1.0.0"));
    CHECK_EQ(vsix::version_in("kosmosisdire.vscode-rant@1.0.0\n", "KosmosisDire.vscode-rant"), std::string("1.0.0"));
}

TEST(listing_without_the_extension_is_empty) {
    CHECK_EQ(vsix::version_in("", id), std::string());
    CHECK_EQ(vsix::version_in("kosmosisdire.vscode-rant-extra@1.0.0\nother.vscode-rant@1.0.0\n", id), std::string());
}

TEST(profiles_start_with_the_default) {
    auto p = vsix::profiles_in(storage);
    CHECK_EQ(p.size(), 4u);
    CHECK_EQ(p[0].id, std::string());
    CHECK_EQ(p[0].name, std::string("Default"));
    CHECK_EQ(p[2].id, std::string("-500e13d6"));
    CHECK_EQ(p[2].name, std::string("Mixed"));
}

TEST(profiles_sharing_the_default_extensions_are_marked) {
    auto p = vsix::profiles_in(storage);
    CHECK(p[1].shared);
    CHECK(!p[0].shared);
    CHECK(!p[3].shared);
}

TEST(unreadable_storage_has_only_the_default) {
    CHECK_EQ(vsix::profiles_in("").size(), 1u);
    CHECK_EQ(vsix::profiles_in("{ \"userDataProfiles\": 3 }").size(), 1u);
    CHECK_EQ(vsix::profiles_in("{ \"userDataProfiles\": [ 1, { \"name\": \"Lost\" } ] }").size(), 1u);
}

TEST(a_folder_gives_its_profile) {
    CHECK_EQ(vsix::folder_profiles(storage, root), Ids({ "-500e13d6" }));
    CHECK_EQ(vsix::folder_profiles(storage, root / "src" / "app"), Ids({ "-500e13d6" }));
}

TEST(nested_folders_give_each_profile) {
    CHECK_EQ(vsix::folder_profiles(storage, root / "cli-rant" / "src"), Ids({ "-500e13d6", "-3b410830" }));
}

TEST(the_default_profile_is_empty) {
    CHECK_EQ(vsix::folder_profiles(storage, fs::path(root.u8string() + " Old")), Ids({ "" }));
}

TEST(a_name_that_only_starts_like_a_folder_is_not_inside_it) {
    CHECK_EQ(vsix::folder_profiles(storage, fs::path(root.u8string() + "X")), Ids());
    CHECK_EQ(vsix::folder_profiles(storage, root.parent_path()), Ids());
}

TEST(folders_without_associations_give_none) {
    CHECK_EQ(vsix::folder_profiles("", root), Ids());
    CHECK_EQ(vsix::folder_profiles("{ \"profileAssociations\": [] }", root), Ids());
}

#ifdef _WIN32
TEST(windows_folders_match_without_case) {
    CHECK_EQ(vsix::folder_profiles(storage, "c:/main/rant"), Ids({ "-500e13d6" }));
}
#endif
