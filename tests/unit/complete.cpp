#include "check.hpp"
#include "complete/complete.hpp"

using Words = std::vector<std::string>;

TEST(narrow_keeps_what_starts_with_the_word_sorted_once) {
    complete::Candidates c{ { "stop", "start", "sub", "start" } };
    CHECK_EQ(complete::narrow(c, "st"), (Words{ "start", "stop" }));
    CHECK_EQ(complete::narrow(c, "x"), Words{});
}

TEST(narrow_ignores_case) {
    complete::Candidates c{ { "Nav", "nav_tool", "talker" } };
    CHECK_EQ(complete::narrow(c, "nav"), (Words{ "Nav", "nav_tool" }));
    CHECK_EQ(complete::narrow(c, "TAL"), (Words{ "talker" }));
}

TEST(narrow_offers_a_group_and_a_node_of_one_name_once_as_typed) {
    complete::Candidates c{ { "Transcoder", "transcoder", "WebExplorer", "web_explorer", "talker" } };
    CHECK_EQ(complete::narrow(c, "transco"), (Words{ "transcoder" }));
    CHECK_EQ(complete::narrow(c, "Transco"), (Words{ "Transcoder" }));
    CHECK_EQ(complete::narrow(c, "web"), (Words{ "web_explorer" }));
    CHECK_EQ(complete::narrow(c, ""), (Words{ "Transcoder", "WebExplorer", "talker" }));
}

TEST(narrow_keeps_paths_that_differ_in_case) {
    complete::Candidates c{ { "/Cam", "/cam" }, true };
    CHECK_EQ(complete::narrow(c, ""), (Words{ "/Cam", "/cam" }));
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
