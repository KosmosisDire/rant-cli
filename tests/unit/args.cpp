#include "app/args.hpp"
#include "app/failure.hpp"
#include "check.hpp"

using app::OptionSpec;

static const std::vector<OptionSpec> specs = {
    { "yes", 'y', "", "" },
    { "domain", 0, "N", "" },
    { "dry-run", 0, "", "" },
};

TEST(words_and_flags_mix) {
    auto a = app::parse_args({ "group", "nav", "-y", "target=red", "--dry-run" }, specs);
    CHECK_EQ(a.words.size(), 3u);
    CHECK_EQ(a.words[2], std::string("target=red"));
    CHECK(a.has("yes"));
    CHECK(a.has("dry-run"));
}

TEST(values_take_the_next_token_or_inline) {
    CHECK_EQ(*app::parse_args({ "--domain", "7" }, specs).get("domain"), std::string("7"));
    CHECK_EQ(*app::parse_args({ "--domain=8" }, specs).get("domain"), std::string("8"));
}

TEST(negative_numbers_are_words) {
    auto a = app::parse_args({ "/speed", "-1.5" }, specs);
    CHECK_EQ(a.words.size(), 2u);
    CHECK_EQ(a.words[1], std::string("-1.5"));
}

TEST(double_dash_ends_options) {
    auto a = app::parse_args({ "--", "-y" }, specs);
    CHECK_EQ(a.words.size(), 1u);
    CHECK(!a.has("yes"));
}

TEST(unknown_and_malformed_options_are_refused) {
    CHECK_THROWS(app::parse_args({ "--nope" }, specs));
    CHECK_THROWS(app::parse_args({ "-q" }, specs));
    CHECK_THROWS(app::parse_args({ "--domain" }, specs));
    CHECK_THROWS(app::parse_args({ "--yes=1" }, specs));
}

TEST(stop_at_word_finds_the_command) {
    size_t used = 0;
    auto a = app::parse_args({ "--domain", "3", "ls", "--json" }, specs, true, &used);
    CHECK_EQ(used, 2u);
    CHECK_EQ(*a.get("domain"), std::string("3"));
}
