#include "app/failure.hpp"
#include "check.hpp"
#include "mesh/values.hpp"

using mesh::json;

/* A node only compiles schemas here, it never sends. */
static rant::Node& node() {
    static rant::Node n("values-test", [] {
        rant::NodeOptions o;
        o.domain = 233;
        o.multicast_interface = "127.0.0.1";
        return o;
    }());
    return n;
}

TEST(text_reads_words_numbers_and_relaxed_json) {
    CHECK_EQ(mesh::parse_text("1.5"), json(1.5));
    CHECK_EQ(mesh::parse_text("-3"), json(-3));
    CHECK_EQ(mesh::parse_text("true"), json(true));
    CHECK_EQ(mesh::parse_text("hello world"), json("hello world"));
    CHECK_EQ(mesh::parse_text("{x: 1, frame: map, tags: [a, 'b c'], \"q\": \"x\\\"y\",}"),
             json({ { "x", 1 }, { "frame", "map" }, { "tags", { "a", "b c" } }, { "q", "x\"y" } }));
    CHECK_THROWS(mesh::parse_text("{x 1}"));
    CHECK_THROWS(mesh::parse_text("[1, 2"));
}

TEST(words_are_one_value_or_assignments) {
    auto a = mesh::parse_words({ "x=1", "pose.y=2", "pts[1].x=3" });
    CHECK_EQ(a.size(), 3u);
    CHECK_EQ(a[2].first, std::string("pts[1].x"));
    CHECK_EQ(a[2].second, json(3));
    auto v = mesh::parse_words({ "hello", "world" });
    CHECK_EQ(v[0].first, std::string(""));
    CHECK_EQ(v[0].second, json("hello world"));
    CHECK_THROWS(mesh::parse_words({ "x=1", "loose" }));
    CHECK_THROWS(mesh::parse_words({}));
}

static json round_trip(const rant::Schema& s, const mesh::Assignments& a) {
    auto bytes = mesh::encode(a, s);
    return mesh::to_json(rant::Bytes(bytes.data(), bytes.size()), s);
}

TEST(every_kind_round_trips) {
    rant::Schema s = node().schema(
        "Pt { x: f32, y: f32 } "
        "Sample { seq: u32, delta: i16, value: f64, name: string, label: string<8>, tags: string<16>[], "
        "nums: i16[], fixed: f64[3], pts: Pt[], mode: enum<u8> { Idle, Run }, on: bool, extra: map }");
    json in = json::parse(R"({"seq":7,"delta":-2,"value":1.5,"name":"long name","label":"short","tags":["a","b"],
        "nums":[1,-2,3],"fixed":[1.0,2.0,3.0],"pts":[{"x":1.0,"y":2.0},{"x":3.0,"y":4.0}],"mode":"Run","on":true,
        "extra":{"k":1,"s":"t","nested":{"b":false},"list":[1,2]}})");
    json out = round_trip(s, { { "", in } });
    CHECK_EQ(out, in);
}

TEST(assignments_reach_nested_and_indexed_fields) {
    rant::Schema s = node().schema("Pt { x: f32, y: f32 } Path { name: string, pts: Pt[], at: Pt }");
    json out = round_trip(s, { { "pts[1].y", json(5) }, { "at.x", json(2) }, { "name", json("p") } });
    CHECK_EQ(out, json::parse(R"({"name":"p","pts":[{"x":0.0,"y":0.0},{"x":0.0,"y":5.0}],"at":{"x":2.0,"y":0.0}})"));
}

TEST(a_bare_root_takes_the_value_itself) {
    CHECK_EQ(round_trip(node().schema("f64"), { { "", json(2.5) } }), json(2.5));
    CHECK_EQ(round_trip(node().schema("string"), { { "", json("hi there") } }), json("hi there"));
    CHECK_EQ(round_trip(node().schema("u32[]"), { { "", json::parse("[1,2]") } }), json::parse("[1,2]"));
}

TEST(mistakes_say_what_was_wanted) {
    rant::Schema s = node().schema("Shape { side: u8, mode: enum<u8> { Idle, Run }, label: string<4> }");
    auto message = [&](const mesh::Assignments& a) {
        try {
            mesh::encode(a, s);
        } catch (const app::Failure& e) {
            return std::string(e.what());
        }
        return std::string("no error");
    };
    CHECK_EQ(message({ { "colour", json(1) } }), std::string("there is no field `colour`, the fields are side, mode, label"));
    CHECK_EQ(message({ { "side", json(-1) } }), std::string("field `side` takes a whole number from 0 up, not -1"));
    CHECK_EQ(message({ { "mode", json("Fly") } }), std::string("field `mode` takes one of Idle, Run, not Fly"));
    CHECK_EQ(message({ { "label", json("too long") } }), std::string("field `label` takes a value that fits it, not too long"));
}

TEST(no_schema_sends_text) {
    auto b = mesh::encode({ { "", json("raw words") } }, rant::Schema());
    CHECK_EQ(std::string(b.begin(), b.end()), std::string("raw words"));
}
