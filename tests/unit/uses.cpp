#include <chrono>
#include <fstream>

#include "app/failure.hpp"
#include "check.hpp"
#include "library/uses.hpp"

namespace fs = std::filesystem;
using library::Use;

static const std::string cpm =
    "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\ninclude(cmake/get_cpm.cmake)\n# CPMAddPackage(NAME rant GIT_TAG v0.0.1)\n"
    "CPMAddPackage(\n  NAME rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG \"v0.0.16\" # pinned\n)\n"
    "add_executable(cam main.cpp)\n";

static std::string replaced(std::string s, const std::string& from, const std::string& to) {
    for (size_t at = s.find(from); at != std::string::npos; at = s.find(from, at + to.size())) s.replace(at, from.size(), to);
    return s;
}

static bool refused(std::string (*f)(const std::string&, const std::string&), const std::string& text) {
    try {
        f(text, "0.0.18");
    } catch (const app::Failure&) {
        return true;
    }
    return false;
}

TEST(cpm_version_is_read_and_moved_keeping_the_layout) {
    auto u = library::cmake_use(cpm);
    CHECK(u && u->how == "CPM" && u->version == std::string("0.0.16"));
    CHECK_EQ(library::cmake_set(cpm, "0.0.18"), replaced(cpm, "\"v0.0.16\"", "\"v0.0.18\""));
}

TEST(short_forms_and_urls_move_too_and_others_are_refused) {
    std::string shorter = "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.16\")\n";
    CHECK_EQ(library::cmake_set(shorter, "0.0.18"), std::string("CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.18\")\n"));
    std::string url = "FetchContent_Declare(rant URL https://github.com/KosmosisDire/Rant/archive/refs/tags/v0.0.16.zip)\n";
    CHECK_EQ(library::cmake_set(url, "0.0.18"), replaced(url, "0.0.16", "0.0.18"));
    CHECK(refused(library::cmake_set, "FetchContent_Declare(rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG main)\n"));
    std::string installed = "find_package(rant 0.0.16 CONFIG REQUIRED)\n";
    auto u = library::cmake_use(installed);
    CHECK(u && u->how == "find_package" && u->version == std::string("0.0.16"));
    CHECK(refused(library::cmake_set, installed));
}

TEST(add_uses_fetchcontent_without_cpm_and_links_the_one_target) {
    std::string text = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\nadd_executable(cam main.cpp)\n";
    std::string hint = "unset";
    CHECK_EQ(library::cmake_add(text, "0.0.18", &hint),
             std::string("cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\ninclude(FetchContent)\nFetchContent_Declare(rant\n"
                         "  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG v0.0.18\n  GIT_SHALLOW TRUE)\n"
                         "FetchContent_MakeAvailable(rant)\n\nadd_executable(cam main.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n"));
    CHECK_EQ(hint, std::string(""));
}

TEST(add_follows_cpm_and_the_plain_link_signature) {
    std::string text = "project(a)\ninclude(cmake/get_cpm.cmake)\nadd_executable(a a.cpp)\ntarget_link_libraries(a m)\n";
    std::string out = library::cmake_add(text, "0.0.18", nullptr);
    CHECK(out.find("include(cmake/get_cpm.cmake)\n\nCPMAddPackage(NAME rant\n") != std::string::npos);
    CHECK(out.find("add_executable(a a.cpp)\ntarget_link_libraries(a rant::rant_host)\n") != std::string::npos);
    std::string hint;
    library::cmake_add("project(a)\nadd_executable(a a.cpp)\nadd_executable(b b.cpp)\n", "0.0.18", &hint);
    CHECK(!hint.empty());
}

TEST(pyproject_pins_move_and_bare_ones_stay) {
    std::string text = "[project]\ndependencies = [\"numpy\", \"rant-middleware>=0.0.16\"]\n";
    CHECK_EQ(library::pyproject_set(text, "0.0.18"), replaced(text, "0.0.16", "0.0.18"));
    CHECK(refused(library::pyproject_set, "dependencies = [\"rant-middleware\"]\n"));
}

TEST(csharp_reference_is_read_moved_pinned_and_added) {
    std::string text = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n"
                       "    <PackageReference Include=\"Rant\" Version=\"0.0.16\" />\n  </ItemGroup>\n</Project>\n";
    auto u = library::csharp_use(text);
    CHECK(u && u->version == std::string("0.0.16"));
    CHECK_EQ(library::csharp_set(text, "0.0.18"), replaced(text, "\"0.0.16\"", "\"[0.0.18]\""));
    std::string child = "<PackageReference Include=\"Rant\">\n  <Version> [0.0.16,) </Version>\n</PackageReference>";
    auto c = library::csharp_use(child);
    CHECK(c && c->version == std::string("0.0.16"));
    CHECK_EQ(library::csharp_set(child, "0.0.18"), replaced(child, "[0.0.16,)", "[0.0.18]"));

    std::string without = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n  </ItemGroup>\n</Project>\n";
    CHECK(library::csharp_add(without, "0.0.18").find("Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>") !=
          std::string::npos);
    CHECK_EQ(library::csharp_add("<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n", "0.0.18"),
             std::string("<Project Sdk=\"Microsoft.NET.Sdk\">\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n"
                         "  </ItemGroup>\n</Project>\n"));
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
    auto uses = library::under(t.root);
    CHECK_EQ(uses.size(), size_t(2));
    CHECK(uses[0].how == "CPM" && uses[0].version == std::string("0.0.16"));
    CHECK(uses[1].how == "venv" && uses[1].version == std::string("0.0.17") && uses[1].dir == t.root / "py");
}

TEST(a_shared_venv_is_listed_once_where_it_is) {
    Scratch t;
    t.write("rant.hcl", "workspace {}\n");
    t.write(".venv/pyvenv.cfg", "home = x\n");
    t.write("io/ft/ft.py", "import rant\n");
    t.write("services/pick/main.py", "from rant import Node\n");
    auto uses = library::under(t.root);
    CHECK_EQ(uses.size(), size_t(1));
    CHECK(!uses.empty() && fs::equivalent(uses[0].dir, t.root));
}
