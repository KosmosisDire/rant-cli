#include "check.hpp"
#include "mesh/schema_text.hpp"

/* A node only compiles schemas here, it never sends. */
static rant::Node& node() {
    static rant::Node n("schema-text-test", [] {
        rant::NodeOptions o;
        o.domain = 234;
        o.multicast_interface = "127.0.0.1";
        return o;
    }());
    return n;
}

static std::string text(const char* schema) { return mesh::schema_text(node().schema(schema), nullptr); }

TEST(a_standard_type_is_its_name) {
    CHECK_EQ(text("Pose"), std::string("Pose"));
    CHECK_EQ(text("f64"), std::string("f64"));
    CHECK_EQ(text("u8[]"), std::string("u8[]"));
}

TEST(a_user_type_prints_one_field_per_line_then_the_types_it_uses) {
    CHECK_EQ(text("Pt { x: f32, y: f32 } "
                  "Route { name: string, pts: Pt[], at: Pose, mode: enum<u8> { Idle, Run }, raw: u8[4], tags: string<8>[] }"),
             std::string("Route {\n"
                         "  name: string\n"
                         "  pts:  Pt[]\n"
                         "  at:   Pose\n"
                         "  mode: enum<u8> { Idle, Run }\n"
                         "  raw:  u8[4]\n"
                         "  tags: string<8>[]\n"
                         "}\n"
                         "\n"
                         "Pt {\n"
                         "  x: f32\n"
                         "  y: f32\n"
                         "}"));
}

TEST(an_anonymous_struct_prints_inline) {
    CHECK_EQ(text("{ a: u8, inner: { b: bool } }"),
             std::string("{\n"
                         "  a:     u8\n"
                         "  inner: {\n"
                         "    b: bool\n"
                         "  }\n"
                         "}"));
}

TEST(no_schema_is_untyped) {
    CHECK_EQ(mesh::schema_text(rant::Schema(), nullptr), std::string("untyped"));
}
