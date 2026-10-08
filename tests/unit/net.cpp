#include "app/failure.hpp"
#include "check.hpp"
#include "net/github.hpp"

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
