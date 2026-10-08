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
    void write(const std::string& rel, const std::string& text) {
        fs::create_directories((root / fs::u8path(rel)).parent_path());
        std::ofstream(root / fs::u8path(rel), std::ios::binary) << text;
    }
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

TEST(names_split_into_words_for_every_case) {
    CHECK_EQ(scaffold::in_case("lidar-driver", "pascal"), std::string("LidarDriver"));
    CHECK_EQ(scaffold::in_case("LidarDriver2", "snake"), std::string("lidar_driver2"));
    CHECK_EQ(scaffold::in_case("lidar_driver", "camel"), std::string("lidarDriver"));
    CHECK_EQ(scaffold::in_case("Big Box", "kebab"), std::string("big-box"));
}

TEST(a_built_in_package_renders_with_its_node_and_never_overwrites) {
    Scratch t;
    scaffold::Made made = scaffold::make({ "package-cpp", {} }, t.root / "cam", "cam", {});
    CHECK_EQ(made.files.size(), size_t(2));
    std::string cmake = t.read("cam/CMakeLists.txt");
    CHECK(cmake.find("project(cam CXX)\n") != std::string::npos);
    CHECK(cmake.find("add_executable(cam cam.cpp)\n") != std::string::npos);
    CHECK(t.read("cam/cam.cpp").find("rant::Node node(\"cam\");") != std::string::npos);
    CHECK(made.next.empty());
    CHECK(failure_of([&] { scaffold::make({ "package-cpp", {} }, t.root / "cam", "cam", {}); }).find("exists already") != std::string::npos);
}

TEST(a_folder_template_takes_params_and_renders_names) {
    Scratch t;
    t.write("tpl/template.hcl", "template {\n  param \"port\" {\n    type = int\n  }\n  next = \"port {{ port }}\"\n}\n");
    t.write("tpl/{{snake(name)}}.txt", "{{ pascal(name) }} on {{ port }}\n## not a statement\n");
    scaffold::Origin tpl{ "", t.root / "tpl" };
    CHECK(failure_of([&] { scaffold::make(tpl, t.root / "out", "Web App", {}); }).find("required param `port`") != std::string::npos);
    scaffold::Made made = scaffold::make(tpl, t.root / "out", "Web App", { "port=8080" });
    CHECK_EQ(t.read("out/web_app.txt"), std::string("WebApp on 8080\n## not a statement\n"));
    CHECK_EQ(made.next, std::string("port 8080"));
    CHECK(failure_of([&] { scaffold::make(tpl, t.root / "out2", "x", { "port=eighty" }); }).find("takes an int") != std::string::npos);
    CHECK(failure_of([&] { scaffold::make({ "", t.root / "nothing" }, t.root / "out3", "x", {}); }).find("not a template") != std::string::npos);
}
