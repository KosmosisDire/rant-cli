#include "check.hpp"
#include "ui/csv.hpp"
#include "ui/live.hpp"
#include "ui/table.hpp"
#include "ui/yaml.hpp"

using json = nlohmann::ordered_json;

static json parse(const char* text) { return json::parse(text); }

TEST(yaml_block_aligns_keys_and_nests) {
    ui::Yaml y(nullptr, false);
    CHECK_EQ(y.block(parse(R"({"seq":3,"pose":{"x":1.5,"yaw":-0.25},"tags":["a","b"],"pts":[{"x":1,"y":2}],"none":[]})")),
             std::string("seq:  3\n"
                         "pose:\n"
                         "  x:   1.50\n"
                         "  yaw: -0.25\n"
                         "tags: [a, b]\n"
                         "pts:\n"
                         "  - x: 1\n"
                         "    y: 2\n"
                         "none: []"));
}

TEST(yaml_floats_take_two_decimals_and_no_negative_zero) {
    ui::Yaml y(nullptr, false);
    CHECK_EQ(y.block(json(2.0)), std::string("2.00"));
    CHECK_EQ(y.block(json(-0.004)), std::string("0.00"));
    CHECK_EQ(y.block(json(1234.5678)), std::string("1234.57"));
    CHECK_EQ(y.block(json(7)), std::string("7"));
}

TEST(yaml_stable_numbers_keep_their_width) {
    ui::Yaml y(nullptr, true);
    CHECK_EQ(y.flow(parse(R"({"x":1.5})")), std::string("{x:  1.50}"));
    CHECK_EQ(y.flow(parse(R"({"x":-12.25})")), std::string("{x: -12.25}"));
    CHECK_EQ(y.flow(parse(R"({"x":1})")), std::string("{x:      1}"));
    CHECK_EQ(y.flow(parse(R"({"x":1.0})")), std::string("{x:   1.00}"));
}

TEST(yaml_quotes_only_what_would_misread) {
    ui::Yaml y(nullptr, false);
    CHECK_EQ(y.block(json("hello world")), std::string("hello world"));
    CHECK_EQ(y.block(json("true")), std::string("\"true\""));
    CHECK_EQ(y.block(json("")), std::string("\"\""));
    CHECK_EQ(y.block(json("12")), std::string("\"12\""));
    CHECK_EQ(y.block(json("a: b")), std::string("\"a: b\""));
    CHECK_EQ(y.block(json("-x")), std::string("\"-x\""));
    CHECK_EQ(y.block(json("two\nlines")), std::string("\"two\\nlines\""));
    CHECK_EQ(y.flow(parse(R"({"k":"a,b"})")), std::string("{k: \"a,b\"}"));
}

TEST(csv_header_once_then_rows) {
    ui::Csv c;
    CHECK_EQ(c.rows(parse(R"({"seq":1,"pose":{"x":0.5},"pts":[{"y":2}],"name":"a,b"})")),
             std::string("seq,pose.x,pts[0].y,name\n1,0.5,2,\"a,b\""));
    CHECK_EQ(c.rows(json{ { "seq", 2 }, { "pose", { { "x", 0.25 } } }, { "name", "say \"hi\"" } }),
             std::string("2,0.25,,\"say \"\"hi\"\"\""));
}

TEST(csv_bare_value) {
    ui::Csv c;
    CHECK_EQ(c.rows(json(1.5)), std::string("value\n1.5"));
}

TEST(clip_counts_visible_characters_only) {
    CHECK_EQ(ui::clip("\x1b[1mabcdef\x1b[0m", 3), std::string("\x1b[1mabc\x1b[0m\x1b[0m"));
    CHECK_EQ(ui::clip("ab", 5), std::string("ab"));
}

using Lines = std::vector<std::string>;

TEST(columns_fill_down_then_across_as_wide_as_fits) {
    std::vector<std::vector<std::string>> cells = { { "a" }, { "b" }, { "c" }, { "d" }, { "e" } };
    CHECK_EQ(ui::columns(cells, 20, ""), (Lines{ "a    c    e", "b    d" }));
    CHECK_EQ(ui::columns(cells, 0, "").size(), size_t(5));
    CHECK_EQ(ui::columns(cells, 1, "").size(), size_t(5));
}

TEST(lists_on_one_grid_line_up_their_columns) {
    std::vector<std::vector<std::vector<std::string>>> lists = { { { "ft" }, { "lector" }, { "visionary" } }, { { "cage-plc" }, { "hmi" } } };
    auto laid = ui::columns(lists, 40, "    ");
    CHECK_EQ(laid[0], (Lines{ "    ft           lector       visionary" }));
    CHECK_EQ(laid[1], (Lines{ "    cage-plc     hmi" }));
    CHECK_EQ(ui::columns(lists, 0, "  ")[0].size(), size_t(3));
}

TEST(columns_line_up_the_parts_of_each_cell) {
    std::vector<std::vector<std::string>> cells = { { "odom", "topic" }, { "rate", "var" }, { "camera/left", "topic" } };
    CHECK_EQ(ui::columns(cells, 40, "  "), (Lines{ "  odom  topic    camera/left  topic", "  rate  var" }));
}
