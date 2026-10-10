#include <chrono>
#include <fstream>
#include <functional>
#include <iterator>

#include "app/failure.hpp"
#include "check.hpp"
#include "scaffold/scaffold.hpp"

namespace fs = std::filesystem;

/* A scratch folder, removed with everything in it. */
struct Scratch {
    fs::path root = fs::temp_directory_path() / ("rant-scaffold-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    std::string read(const std::string& rel) {
        std::ifstream in(root / fs::u8path(rel), std::ios::binary);
        return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    ~Scratch() {
        std::error_code ec;
        fs::remove_all(root, ec);
    }
};

static std::string failure_of(const std::function<void()>& f) {
    try {
        f();
    } catch (const app::Failure& e) {
        return e.what();
    }
    return "";
}

TEST(a_preset_writes_its_folders_with_the_name_and_never_overwrites) {
    Scratch t;
    auto files = scaffold::plan("package-cpp", t.root / "cam", { { "name", "cam" } });
    CHECK_EQ(files.size(), size_t(2));
    scaffold::write(files);
    std::string cmake = t.read("cam/CMakeLists.txt");
    CHECK(cmake.find("project(cam CXX)\n") != std::string::npos);
    CHECK(cmake.find("add_executable(cam cam.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n") != std::string::npos);
    CHECK(t.read("cam/cam.cpp").find("rant::Node node(\"cam\");") != std::string::npos);
    CHECK(failure_of([&] { scaffold::plan("package-cpp", t.root / "cam", { { "name", "cam" } }); }).find("exists already") != std::string::npos);
}

TEST(a_csharp_preset_targets_the_framework_given) {
    Scratch t;
    auto files = scaffold::plan("package-csharp", t.root / "panel", { { "name", "panel" }, { "framework", "net10.0" } });
    scaffold::write(files);
    CHECK(t.read("panel/panel.csproj").find("<TargetFramework>net10.0</TargetFramework>") != std::string::npos);
}

TEST(a_preset_that_does_not_exist_is_an_error) {
    Scratch t;
    CHECK(failure_of([&] { scaffold::plan("nothing", t.root, { { "name", "x" } }); }).find("no preset `nothing`") != std::string::npos);
}
