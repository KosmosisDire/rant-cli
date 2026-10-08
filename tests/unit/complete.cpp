#include "check.hpp"
#include "complete/complete.hpp"

using Words = std::vector<std::string>;

TEST(narrow_keeps_what_starts_with_the_word_sorted_once) {
    complete::Candidates c{ { "stop", "start", "sub", "start" } };
    CHECK_EQ(complete::narrow(c, "st"), (Words{ "start", "stop" }));
    CHECK_EQ(complete::narrow(c, "x"), Words{});
}

TEST(narrow_offers_paths_one_segment_at_a_time) {
    complete::Candidates c{ { "/camera/left", "/camera/right", "/odom", "/odom/rate" }, true };
    CHECK_EQ(complete::narrow(c, ""), (Words{ "/camera/", "/odom", "/odom/" }));
    CHECK_EQ(complete::narrow(c, "/cam"), (Words{ "/camera/" }));
    CHECK_EQ(complete::narrow(c, "/camera/"), (Words{ "/camera/left", "/camera/right" }));
}

TEST(narrow_leaves_slashes_alone_outside_paths) {
    complete::Candidates c{ { "demo/sensor", "sensor" } };
    CHECK_EQ(complete::narrow(c, ""), (Words{ "demo/sensor", "sensor" }));
}

TEST(params_offer_keys_then_options) {
    config::GroupInfo g;
    g.params.push_back({ "target", "string", "red", { "red", "blue" }, "" });
    g.params.push_back({ "fast", "bool", "false", {}, "" });
    g.params.push_back({ "speed", "float", "1", {}, "" });
    CHECK_EQ(complete::params(g, "", {}), (Words{ "target=", "fast=", "speed=" }));
    CHECK_EQ(complete::params(g, "", { "fast=true" }), (Words{ "target=", "speed=" }));
    CHECK_EQ(complete::params(g, "target=", {}), (Words{ "target=red", "target=blue" }));
    CHECK_EQ(complete::params(g, "fast=", {}), (Words{ "fast=true", "fast=false" }));
    CHECK_EQ(complete::params(g, "speed=", {}), Words{});
}
