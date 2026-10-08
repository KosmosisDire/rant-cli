#include <filesystem>
#include <fstream>

#include "check.hpp"
#include "state/state.hpp"

namespace fs = std::filesystem;

static fs::path scratch(const char* name) {
    fs::path p = fs::temp_directory_path() / ("rant-unit-" + std::string(name));
    fs::remove_all(p);
    return p;
}

static state::Instance instance(const std::string& name, const std::string& root) {
    state::Instance i;
    i.name = name;
    i.type = "pkg/" + name;
    i.argv = { "x", "--flag" };
    i.env = { { "K", "V" } };
    i.cwd = "/w/pkg";
    i.log = "/w/logs/" + name + ".log";
    i.roots = { root };
    return i;
}

TEST(root_keys_name_the_params) {
    CHECK_EQ((state::Root{ "group", "nav", { { "target", "red" }, { "a", "1" } } }).key(), std::string("group nav a=1 target=red"));
    CHECK_EQ((state::Root{ "node", "talker", {} }).key(), std::string("node talker"));
}

TEST(save_and_load_round_trip) {
    auto dir = scratch("state-roundtrip");
    {
        state::Store s(dir);
        s.state().roots.push_back({ "node", "a", {} });
        s.state().instances.push_back(instance("a", "node a"));
        s.save();
    }
    state::Store s(dir);
    CHECK_EQ(s.state().roots.size(), 1u);
    CHECK_EQ(s.state().instances.size(), 1u);
    CHECK(s.state().instances[0].same_spec(instance("a", "node a")));
    CHECK(s.state().instance("a") != nullptr);
    CHECK(s.state().root("node a") != nullptr);
}

TEST(prune_drops_dead_instances_and_their_node_roots_only) {
    state::State st;
    st.roots.push_back({ "node", "a", {} });
    st.roots.push_back({ "group", "nav", {} });
    st.instances.push_back(instance("a", "node a"));       /* pid 0: never alive */
    st.instances.push_back(instance("b", "group nav"));
    auto dropped = state::prune(st);
    CHECK_EQ(dropped.size(), 2u);
    CHECK(st.instances.empty());
    CHECK_EQ(st.roots.size(), 1u);
    CHECK_EQ(st.roots[0].kind, std::string("group"));
}

TEST(a_damaged_file_is_reported) {
    auto dir = scratch("state-damaged");
    fs::create_directories(dir);
    { std::ofstream(dir / "state") << "{ not json"; }
    CHECK_THROWS(state::Store(dir));
}
