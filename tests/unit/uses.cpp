#include <chrono>
#include <fstream>

#include "app/failure.hpp"
#include "check.hpp"
#include "library/uses.hpp"

namespace fs = std::filesystem;
using library::Use;

static const library::Library rant{ "rant", "KosmosisDire/Rant", { "rant::rant_host", "rant-middleware", "Rant", "rant-middleware" } };
/* names that start as Rant's do, so a match must end where the name does */
static const library::Library extra{ "extra", "KosmosisDire/Rant-extra", { "extra::extra", "rant-middleware-extra", "Rant.Extra", "rant-middleware-extra" } };

static const std::string cpm =
    "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\ninclude(cmake/get_cpm.cmake)\n# CPMAddPackage(NAME rant GIT_TAG v0.0.1)\n"
    "CPMAddPackage(\n  NAME rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG \"v0.0.16\" # pinned\n)\n"
    "add_executable(cam main.cpp)\n";

static std::string replaced(std::string s, const std::string& from, const std::string& to) {
    for (size_t at = s.find(from); at != std::string::npos; at = s.find(from, at + to.size())) s.replace(at, from.size(), to);
    return s;
}

static bool refused(std::string (*f)(const std::string&, const library::Library&, const std::string&), const std::string& text) {
    try {
        f(text, rant, "0.0.18");
    } catch (const app::Failure&) {
        return true;
    }
    return false;
}

TEST(cpm_version_is_read_and_moved_keeping_the_layout) {
    auto u = library::cmake_use(cpm, rant);
    CHECK(u && u->how == "CPM" && u->version == std::string("0.0.16"));
    CHECK_EQ(library::cmake_set(cpm, rant, "0.0.18"), replaced(cpm, "\"v0.0.16\"", "\"v0.0.18\""));
}

TEST(short_forms_and_urls_move_too_and_others_are_refused) {
    std::string shorter = "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.16\")\n";
    CHECK_EQ(library::cmake_set(shorter, rant, "0.0.18"), std::string("CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.18\")\n"));
    std::string url = "FetchContent_Declare(rant URL https://github.com/KosmosisDire/Rant/archive/refs/tags/v0.0.16.zip)\n";
    CHECK_EQ(library::cmake_set(url, rant, "0.0.18"), replaced(url, "0.0.16", "0.0.18"));
    CHECK(refused(library::cmake_set, "FetchContent_Declare(rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG main)\n"));
    std::string installed = "find_package(rant 0.0.16 CONFIG REQUIRED)\n";
    auto u = library::cmake_use(installed, rant);
    CHECK(u && u->how == "find_package" && u->version == std::string("0.0.16"));
    CHECK(refused(library::cmake_set, installed));
}

TEST(add_uses_fetchcontent_without_cpm_and_links_the_one_target) {
    std::string text = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\nadd_executable(cam main.cpp)\n";
    std::string hint = "unset";
    CHECK_EQ(library::cmake_add(text, rant, "v0.0.18", &hint),
             std::string("cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\ninclude(FetchContent)\nFetchContent_Declare(rant\n"
                         "  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG v0.0.18\n  GIT_SHALLOW TRUE)\n"
                         "FetchContent_MakeAvailable(rant)\n\nadd_executable(cam main.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n"));
    CHECK_EQ(hint, std::string(""));
}

TEST(add_follows_cpm_and_the_plain_link_signature) {
    std::string text = "project(a)\ninclude(cmake/get_cpm.cmake)\nadd_executable(a a.cpp)\ntarget_link_libraries(a m)\n";
    std::string out = library::cmake_add(text, rant, "v0.0.18", nullptr);
    CHECK(out.find("include(cmake/get_cpm.cmake)\n\nCPMAddPackage(NAME rant\n") != std::string::npos);
    CHECK(out.find("add_executable(a a.cpp)\ntarget_link_libraries(a rant::rant_host)\n") != std::string::npos);
    std::string hint;
    library::cmake_add("project(a)\nadd_executable(a a.cpp)\nadd_executable(b b.cpp)\n", rant, "v0.0.18", &hint);
    CHECK(!hint.empty());
}

TEST(a_node_joins_after_the_last_target_and_never_twice) {
    std::string text = "project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n\ninstall(TARGETS a)\n";
    CHECK_EQ(library::cmake_add_node(text, "lidar", "nodes/lidar.cpp"),
             std::string("project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n"
                         "add_executable(lidar nodes/lidar.cpp)\ntarget_link_libraries(lidar PRIVATE rant::rant_host)\n\ninstall(TARGETS a)\n"));
    CHECK_EQ(library::cmake_add_node("project(b)", "b", "b.cpp"),
             std::string("project(b)\n\nadd_executable(b b.cpp)\ntarget_link_libraries(b PRIVATE rant::rant_host)\n"));
    bool refused = false;
    try {
        library::cmake_add_node(text, "a", "a2.cpp");
    } catch (const app::Failure&) {
        refused = true;
    }
    CHECK(refused);
}

TEST(pyproject_pins_move_and_bare_ones_stay) {
    std::string text = "[project]\ndependencies = [\"numpy\", \"rant-middleware>=0.0.16\"]\n";
    CHECK_EQ(library::pyproject_set(text, rant, "0.0.18"), replaced(text, "0.0.16", "0.0.18"));
    CHECK(refused(library::pyproject_set, "dependencies = [\"rant-middleware\"]\n"));
}

TEST(csharp_reference_is_read_moved_pinned_and_added) {
    std::string text = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n"
                       "    <PackageReference Include=\"Rant\" Version=\"0.0.16\" />\n  </ItemGroup>\n</Project>\n";
    auto u = library::csharp_use(text, rant);
    CHECK(u && u->version == std::string("0.0.16"));
    CHECK_EQ(library::csharp_set(text, rant, "0.0.18"), replaced(text, "\"0.0.16\"", "\"[0.0.18]\""));
    std::string child = "<PackageReference Include=\"Rant\">\n  <Version> [0.0.16,) </Version>\n</PackageReference>";
    auto c = library::csharp_use(child, rant);
    CHECK(c && c->version == std::string("0.0.16"));
    CHECK_EQ(library::csharp_set(child, rant, "0.0.18"), replaced(child, "[0.0.16,)", "[0.0.18]"));

    std::string without = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n  </ItemGroup>\n</Project>\n";
    CHECK(library::csharp_add(without, rant, "0.0.18").find("Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>") !=
          std::string::npos);
    CHECK_EQ(library::csharp_add("<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n", rant, "0.0.18"),
             std::string("<Project Sdk=\"Microsoft.NET.Sdk\">\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n"
                         "  </ItemGroup>\n</Project>\n"));
}

TEST(cargo_dependency_is_read_moved_and_added) {
    std::string git = "[package]\nname = \"arm\"\n\n[dependencies]\nserde = \"1\"\n"
                      "rant-middleware = { git = \"https://github.com/KosmosisDire/Rant.git\", tag = \"crate-v0.0.24\" }\n";
    auto u = library::cargo_use(git, rant);
    CHECK(u && u->how == "Cargo" && u->version == std::string("0.0.24"));
    CHECK_EQ(library::cargo_set(git, rant, "0.0.25"), replaced(git, "crate-v0.0.24", "crate-v0.0.25"));
    std::string plain_tag = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { git = \"x\", tag = \"v0.0.24\" }\n";
    CHECK_EQ(library::cargo_set(plain_tag, rant, "0.0.25"), replaced(plain_tag, "\"v0.0.24\"", "\"v0.0.25\""));

    std::string plain = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = \"=0.0.24\"\nother = \"0.0.24\"\n";
    CHECK_EQ(library::cargo_set(plain, rant, "0.0.25"), replaced(plain, "rant-middleware = \"=0.0.24\"", "rant-middleware = \"=0.0.25\""));
    std::string renamed = "[package]\nname = \"arm\"\n[dependencies]\nrant = { package = \"rant-middleware\", version = \"0.0.24\" }\n";
    CHECK(library::cargo_use(renamed, rant)->version == std::string("0.0.24"));
    std::string table = "[package]\nname = \"arm\"\n[dependencies.rant-middleware]\ngit = \"x\"\ntag = \"v0.0.24\"\n\n[features]\ntag = \"v9.9.9\"\n";
    CHECK_EQ(library::cargo_set(table, rant, "0.0.25"), replaced(table, "v0.0.24", "v0.0.25"));
    std::string root = "[workspace]\nmembers = [\"arm\"]\n[workspace.dependencies]\nrant-middleware = { git = \"x\", tag = \"v0.0.24\" }\n";
    CHECK(library::cargo_use(root, rant)->version == std::string("0.0.24"));

    std::string member = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { workspace = true }\n";
    auto m = library::cargo_use(member, rant);
    CHECK(m && m->how == "Cargo workspace" && !m->wanted);
    CHECK(refused(library::cargo_set, member));
    CHECK(refused(library::cargo_set, "[package]\nname = \"a\"\n[dependencies]\nrant-middleware = { path = \"../rant\" }\n"));
    CHECK(!library::cargo_use("[package]\nname = \"a\"\n[dev-dependencies]\nrant-middleware = \"0.0.24\"\n", rant));

    std::string line = "rant-middleware = { git = \"https://github.com/KosmosisDire/Rant.git\", tag = \"crate-v0.0.24\" }\n";
    CHECK_EQ(library::cargo_add("[package]\nname = \"arm\"\n\n[dependencies]\nserde = \"1\"\n", rant, "0.0.24"),
             "[package]\nname = \"arm\"\n\n[dependencies]\n" + line + "serde = \"1\"\n");
    CHECK_EQ(library::cargo_add("[package]\nname = \"arm\"\n", rant, "0.0.24"), "[package]\nname = \"arm\"\n\n[dependencies]\n" + line);
    CHECK(refused(library::cargo_add, "[workspace]\nmembers = []\n"));
}

TEST(another_library_is_told_apart_by_its_own_names) {
    std::string cmake = "CPMAddPackage(NAME extra GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git GIT_TAG v1.2.0)\n";
    CHECK(!library::cmake_use(cmake, rant));
    auto u = library::cmake_use(cmake, extra);
    CHECK(u && u->library == "extra" && u->version == std::string("1.2.0"));
    std::string linked = "project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n";
    std::string added = library::cmake_add(linked, extra, "v1.2.0", nullptr);
    CHECK(added.find("FetchContent_Declare(extra\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git\n  GIT_TAG v1.2.0\n") != std::string::npos);
    CHECK(added.find("target_link_libraries(a PRIVATE extra::extra)\n") != std::string::npos);

    std::string py = "dependencies = [\"rant-middleware-extra==1.2.0\", \"rant-middleware>=0.0.16\"]\n";
    CHECK_EQ(library::pyproject_set(py, rant, "0.0.18"), replaced(py, "0.0.16", "0.0.18"));
    CHECK_EQ(library::pyproject_set(py, extra, "1.3.0"), replaced(py, "1.2.0", "1.3.0"));

    std::string cargo = "[package]\nname = \"a\"\n[dependencies]\nrant-middleware-extra = { git = \"x\", tag = \"crate-v1.2.0\" }\n";
    CHECK(!library::cargo_use(cargo, rant));
    auto c = library::cargo_use(cargo, extra);
    CHECK(c && c->version == std::string("1.2.0"));
    CHECK_EQ(library::cargo_add("[package]\nname = \"a\"\n", extra, "1.2.0"),
             std::string("[package]\nname = \"a\"\n\n[dependencies]\nrant-middleware-extra = { git = \"https://github.com/KosmosisDire/Rant-extra.git\", tag = \"crate-v1.2.0\" }\n"));

    std::string cs = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Rant.Extra\" Version=\"1.2.0\" />\n  </ItemGroup>\n</Project>\n";
    CHECK(!library::csharp_use(cs, rant));
    CHECK_EQ(library::csharp_set(cs, extra, "1.3.0"), replaced(cs, "\"1.2.0\"", "\"[1.3.0]\""));
}

/* A scratch folder, removed with everything in it. */
struct Scratch {
    fs::path root = fs::temp_directory_path() / ("rant-uses-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    void write(const std::string& rel, const std::string& text) {
        fs::create_directories((root / rel).parent_path());
        std::ofstream(root / rel, std::ios::binary) << text;
    }
    ~Scratch() {
        std::error_code ec;
        fs::remove_all(root, ec);
    }
};

TEST(a_walk_lists_each_use_by_folder) {
    Scratch t;
    t.write("cam/CMakeLists.txt", cpm);
    t.write("py/node.py", "import rant\n");
    t.write("py/.venv/pyvenv.cfg", "home = x\n");
    t.write("py/.venv/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
    t.write("other/readme.md", "x");
    auto uses = library::under(t.root, { rant });
    CHECK_EQ(uses.size(), size_t(2));
    CHECK(uses[0].how == "CPM" && uses[0].version == std::string("0.0.16"));
    CHECK(uses[1].how == "venv" && uses[1].version == std::string("0.0.17") && uses[1].dir == t.root / "py");
}

TEST(a_walk_lists_each_library_a_venv_holds) {
    Scratch t;
    t.write("py/node.py", "import rant\n");
    t.write("py/.venv/pyvenv.cfg", "home = x\n");
    t.write("py/.venv/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
    t.write("py/.venv/Lib/site-packages/Rant_Middleware_Extra-1.2.0.dist-info/METADATA", "x");
    t.write("tool/tool.py", "print(1)\n");
    auto uses = library::under(t.root, { rant, extra });
    CHECK_EQ(uses.size(), size_t(2));
    CHECK(uses.size() == 2 && uses[0].library == "rant" && uses[0].version == std::string("0.0.17"));
    CHECK(uses.size() == 2 && uses[1].library == "extra" && uses[1].version == std::string("1.2.0"));
    CHECK(library::in_folder(t.root / "tool", { rant, extra }).empty());
    CHECK_EQ(library::in_folder(t.root / "tool", { extra }, true).size(), size_t(1));
}

TEST(a_shared_venv_is_listed_once_where_it_is) {
    Scratch t;
    t.write("workspace.rant", "");
    t.write(".venv/pyvenv.cfg", "home = x\n");
    t.write("io/ft/ft.py", "import rant\n");
    t.write("services/pick/main.py", "from rant import Node\n");
    auto uses = library::under(t.root, { rant });
    CHECK_EQ(uses.size(), size_t(1));
    CHECK(!uses.empty() && fs::equivalent(uses[0].dir, t.root));
}

TEST(every_venv_holding_a_library_is_listed_by_the_workspace_ignore_rules) {
    Scratch t;
    t.write("workspace.rant", "ignore = [\"old/**\"]\n");
    t.write(".gitignore", ".venv/\ntools/\n");
    t.write(".venv/pyvenv.cfg", "home = x\n");
    t.write(".venv/Lib/site-packages/rant_middleware-0.0.18.dist-info/METADATA", "x");
    t.write("tools/env/pyvenv.cfg", "home = x\n");
    t.write("tools/env/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
    t.write("tools/bare/pyvenv.cfg", "home = x\n");
    t.write("old/env/pyvenv.cfg", "home = x\n");
    t.write("old/env/Lib/site-packages/rant_middleware-0.0.16.dist-info/METADATA", "x");
    auto uses = library::under(t.root, { rant });
    CHECK_EQ(uses.size(), size_t(2));
    CHECK(uses.size() == 2 && uses[0].version == std::string("0.0.18") && uses[0].wanted);
    CHECK(uses.size() == 2 && uses[1].version == std::string("0.0.17") && fs::equivalent(uses[1].file, t.root / "tools" / "env"));
}

TEST(cmake_source_is_the_first_source_of_the_target) {
    const std::string text =
        "add_executable(cam WIN32 \"src/cam main.cpp\" src/cam.h)  # cam\n"
        "add_executable(lidar ${COMMON} $<$<CONFIG:Debug>:debug.cpp> src/lidar.cc)\n"
        "add_library(util src/util.cpp)\n"
        "add_executable(arm)\ntarget_sources(arm PRIVATE src/arm.cpp)\n";
    CHECK_EQ(library::cmake_source(text, "cam").value_or(""), std::string("src/cam main.cpp"));
    CHECK_EQ(library::cmake_source(text, "lidar").value_or(""), std::string("src/lidar.cc"));
    CHECK_EQ(library::cmake_source(text, "arm").value_or(""), std::string("src/arm.cpp"));
    CHECK(!library::cmake_source(text, "util"));
    CHECK(!library::cmake_source("# add_executable(cam main.cpp)\n", "cam"));
}

TEST(csharp_main_is_found_in_its_forms) {
    CHECK(library::csharp_main("class P { static void Main(string[] a) {} }"));
    CHECK(library::csharp_main("public static async Task<int> Main() => 0;"));
    CHECK(!library::csharp_main("var node = new Rant.Node(\"panel\");"));
}
