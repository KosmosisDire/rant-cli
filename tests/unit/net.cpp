#include "app/failure.hpp"
#include "check.hpp"
#include "net/github.hpp"
#include "packages/packages.hpp"

TEST(a_release_reads_its_tag_and_assets) {
    net::Release r = net::parse_release(
        R"({"tag_name":"v0.0.17","assets":[{"name":"rant.h","browser_download_url":"https://x/rant.h"},{"name":"odd"}]})");
    CHECK_EQ(r.tag, std::string("v0.0.17"));
    CHECK_EQ(r.version, std::string("0.0.17"));
    CHECK_EQ(r.assets.size(), size_t(1));
    CHECK(r.asset("rant.h") && r.asset("rant.h")->url == "https://x/rant.h");
    CHECK(!r.asset("odd"));
}

TEST(an_answer_without_a_tag_is_refused) {
    bool refused = false;
    try {
        net::parse_release("{}");
    } catch (const app::Failure&) {
        refused = true;
    }
    CHECK(refused);
}

TEST(a_tag_without_a_v_is_its_version) {
    CHECK_EQ(net::parse_release(R"({"tag_name":"1.2.0"})").version, std::string("1.2.0"));
    CHECK_EQ(net::parse_release(R"({"tag_name":"V2.0.0-rc.1"})").version, std::string("2.0.0-rc.1"));
    CHECK_EQ(net::parse_release(R"({"tag_name":"vintage"})").version, std::string("vintage"));
}

TEST(versions_order_number_by_number_and_pre_releases_first) {
    CHECK(net::newer("0.0.12", "0.0.9"));
    CHECK(net::newer("v1.0.0", "0.9.9"));
    CHECK(!net::newer("1.2", "1.2.0") && !net::newer("1.2.0", "1.2"));
    CHECK(net::newer("1.2.0", "1.2.0-rc.1"));
    CHECK(net::newer("1.2.0-rc.1", "1.2.0-beta.2"));
    CHECK(net::newer("1.2.0-beta.2", "1.2.0-beta.1"));
    CHECK(net::newer("1.2.0-beta.1", "1.2.0-alpha"));
    CHECK(net::newer("1.2.0b1", "1.2.0a3"));
    CHECK(net::newer("1.2.0rc1", "1.2.0b2"));
    CHECK(net::newer("1.2.0a1", "1.2.0.dev4"));
    CHECK(net::newer("1.2.0.post1", "1.2.0"));
    CHECK(net::newer("1.2.1-beta.1", "1.2.0"));
    CHECK(!net::newer("1.2.0+build.5", "1.2.0"));
}

TEST(a_release_carries_files_by_their_names) {
    net::Release r = net::parse_release(R"({"tag_name":"v1.2.0-beta.1","assets":[
        {"name":"bridge-1.2.0-beta.1-win-x64.tar.gz","browser_download_url":"u1"},
        {"name":"bridge-1.2.0-beta.1-linux-x64.tar.gz","browser_download_url":"u2"},
        {"name":"bridge-1.2.0-beta.1.vsix","browser_download_url":"u3"},
        {"name":"bridge-tools-1.2.0-beta.1-win-x64.tar.gz","browser_download_url":"u4"},
        {"name":"rant-1.2.0-beta.1-win-x64.exe","browser_download_url":"u5"}]})");
    packages::Carried c = packages::carried(r, "bridge", "win-x64");
    CHECK(c.archive && c.archive->url == "u1");
    CHECK(c.extension && c.extension->url == "u3");
    CHECK_EQ(c.platforms.size(), size_t(2));
    CHECK(c.platforms[0] == "win-x64" && c.platforms[1] == "linux-x64");
    packages::Carried mac = packages::carried(r, "bridge", "osx-arm64");
    CHECK(!mac.archive && mac.extension);
    CHECK(!packages::carried(r, "rant", "win-x64").archive);
}
