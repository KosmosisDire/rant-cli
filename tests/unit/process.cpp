#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <thread>

#include "check.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"

namespace fs = std::filesystem;

/* A scratch directory per run, removed at the end. */
static fs::path scratch(const char* name) {
    fs::path p = fs::temp_directory_path() / ("rant-unit-" + std::string(name));
    fs::remove_all(p);
    fs::create_directories(p);
    return p;
}

/* The CMake running these tests: a real program on every OS that can sleep and exit with
 * a code, with no shell. */
static std::string cmake() { return RANT_TEST_CMAKE; }

TEST(run_returns_the_exit_code) {
    auto dir = scratch("run");
    CHECK_EQ(process::run({ { cmake(), "-E", "true" }, dir, {} }), 0);
    CHECK_EQ(process::run({ { cmake(), "-E", "false" }, dir, {} }), 1);
}

TEST(run_refuses_a_missing_program) {
    CHECK_THROWS(process::run({ { "no-such-program-rant-test" }, fs::current_path(), {} }));
}

TEST(environment_applies_overrides_and_leaves_out_unset_names) {
    auto env = process::environment({ {}, {}, { { "RANT_UNIT_X", "1" } }, { "PATH" } });
    bool found = false, path = false;
    for (auto& e : env) {
        found |= e == "RANT_UNIT_X=1";
        path |= e.size() > 5 && (e.compare(0, 5, "PATH=") == 0 || e.compare(0, 5, "Path=") == 0);
    }
    CHECK(found);
    CHECK(!path);
}

TEST(find_program_searches_path_and_takes_relative_paths) {
    CHECK(process::find_program(fs::path(cmake()).filename().u8string(), fs::current_path()).has_value() ||
          !std::getenv("PATH"));
    auto dir = scratch("find");
    CHECK(!process::find_program("./missing", dir).has_value());
}

TEST(detached_start_alive_and_stop) {
    auto dir = scratch("detached");
    process::Tracking t = process::start_detached({ { cmake(), "-E", "sleep", "30" }, dir, {} }, dir / "out.log",
                                                  "Local\\rant-unit-detached");
    CHECK(t.pid != 0);
    CHECK(process::alive(t));
    CHECK(process::owns(t, t.pid));
    process::Stopped how = process::stop(t, std::chrono::seconds(3));
    CHECK(how == process::Stopped::Gracefully);
    CHECK(!process::alive(t));
    CHECK(process::stop(t, std::chrono::seconds(1)) == process::Stopped::AlreadyGone);
}

TEST(a_process_that_exits_is_not_alive) {
    auto dir = scratch("exits");
    process::Tracking t = process::start_detached({ { cmake(), "-E", "true" }, dir, {} }, dir / "out.log",
                                                  "Local\\rant-unit-exits");
    for (int i = 0; i < 100 && process::alive(t); i++) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    CHECK(!process::alive(t));
}

TEST(a_child_tells_its_exit_code) {
    auto dir = scratch("child");
    process::Child ok = process::start_child({ { cmake(), "-E", "true" }, dir, {} }, dir / "ok.log", "Local\\rant-unit-child-ok");
    process::Child bad = process::start_child({ { cmake(), "-E", "false" }, dir, {} }, dir / "bad.log", "Local\\rant-unit-child-bad");
    process::Child slow = process::start_child({ { cmake(), "-E", "sleep", "30" }, dir, {} }, dir / "slow.log", "Local\\rant-unit-child-slow");
    std::optional<int> a, b;
    for (int i = 0; i < 200 && !(a && b); i++) {
        if (!a) a = ok.ended();
        if (!b) b = bad.ended();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    CHECK(a == 0);
    CHECK(b == 1);
    CHECK(!slow.ended().has_value());
    process::stop(slow.tracking(), std::chrono::seconds(3));
    std::optional<int> c;
    for (int i = 0; i < 200 && !c; i++) {
        c = slow.ended();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    CHECK(c.has_value());
}

TEST(an_untrackable_id_is_never_signalled) {
    for (uint64_t pid : { 0ull, 1ull }) {
        process::Tracking t;
        t.pid = pid;
        CHECK(!process::alive(t));
        CHECK(process::stop(t, std::chrono::milliseconds(10)) == process::Stopped::AlreadyGone);
    }
}
