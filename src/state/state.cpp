#include "state/state.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "util/home.hpp"

namespace state {

using json = nlohmann::json;

std::string Root::key() const {
    std::string k = kind + " " + name;
    for (auto& [p, v] : params) k += " " + p + "=" + v;
    return k;
}

bool Instance::same_spec(const Instance& o) const {
    return type == o.type && argv == o.argv && env == o.env && cwd == o.cwd;
}

Instance* State::instance(const std::string& name) {
    for (auto& i : instances)
        if (i.name == name) return &i;
    return nullptr;
}

Root* State::root(const std::string& key) {
    for (auto& r : roots)
        if (r.key() == key) return &r;
    return nullptr;
}

static const int FORMAT = 1;

static State from_json(const json& j) {
    State s;
    if (j.value("format", 0) != FORMAT) return s;
    for (auto& r : j.at("roots"))
        s.roots.push_back({ r.at("kind"), r.at("name"), r.at("params").get<std::map<std::string, std::string>>() });
    for (auto& i : j.at("instances")) {
        Instance in;
        in.name = i.at("name");
        in.type = i.at("type");
        in.argv = i.at("argv").get<std::vector<std::string>>();
        in.env = i.at("env").get<std::map<std::string, std::string>>();
        in.cwd = i.at("cwd");
        in.log = i.at("log");
        in.tracking.pid = i.at("pid");
        in.tracking.start_time = i.at("start_time");
        in.tracking.job = i.at("job");
        in.roots = i.at("roots").get<std::vector<std::string>>();
        in.group = i.value("group", "");
        in.expect_mesh = i.value("expect_mesh", true);
        in.restart = i.value("restart", "never");
        in.restarts = i.value("restarts", 0);
        s.instances.push_back(std::move(in));
    }
    return s;
}

static json to_json(const State& s) {
    json roots = json::array(), instances = json::array();
    for (auto& r : s.roots) roots.push_back({ { "kind", r.kind }, { "name", r.name }, { "params", r.params } });
    for (auto& i : s.instances)
        instances.push_back({ { "name", i.name },
                              { "type", i.type },
                              { "argv", i.argv },
                              { "env", i.env },
                              { "cwd", i.cwd },
                              { "log", i.log },
                              { "pid", i.tracking.pid },
                              { "start_time", i.tracking.start_time },
                              { "job", i.tracking.job },
                              { "roots", i.roots },
                              { "group", i.group },
                              { "expect_mesh", i.expect_mesh },
                              { "restart", i.restart },
                              { "restarts", i.restarts } });
    return { { "format", FORMAT }, { "roots", roots }, { "instances", instances } };
}

static const fs::path& made(const fs::path& dir) {
    fs::create_directories(dir);
    return dir;
}

Store::Store(const fs::path& data_dir, bool wait) : path_(data_dir / "state"), lock_(made(data_dir) / "state.lock", wait) {
    if (!lock_.held()) return;
    std::ifstream f(path_, std::ios::binary);
    if (!f) return;
    try {
        state_ = from_json(json::parse(f));
    } catch (const std::exception&) {
        throw app::Failure(path_.u8string() + " is damaged, delete it to start over (running nodes are then forgotten)");
    }
}

void Store::save() {
    fs::path tmp = path_;
    tmp += ".tmp";
    {
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        f << to_json(state_).dump(2) << '\n';
        if (!f) throw app::Failure("cannot write " + tmp.u8string());
    }
    fs::rename(tmp, path_);
}

std::vector<std::string> prune(State& s, bool restarting) {
    std::vector<std::string> dropped;
    for (auto it = s.instances.begin(); it != s.instances.end();) {
        if (process::alive(it->tracking) || (restarting && it->restart != "never")) {
            ++it;
            continue;
        }
        dropped.push_back(it->name);
        it = s.instances.erase(it);
    }
    s.roots.erase(std::remove_if(s.roots.begin(), s.roots.end(),
                                 [&](const Root& r) { return r.kind == "node" && !s.instance(r.name); }),
                  s.roots.end());
    return dropped;
}

static fs::path registry() { return util::rant_home() / "workspaces"; }

void remember(const fs::path& data_dir) {
    fs::path entry = registry() / util::path_key(data_dir);
    std::error_code ec;
    if (fs::exists(entry, ec)) return;
    fs::create_directories(entry.parent_path(), ec);
    std::ofstream(entry, std::ios::binary) << data_dir.u8string();
}

std::vector<fs::path> remembered() {
    std::vector<fs::path> out;
    std::error_code ec;
    for (auto& e : fs::directory_iterator(registry(), ec)) {
        std::string text;
        {
            std::ifstream f(e.path(), std::ios::binary);
            text.assign(std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>());
        }
        fs::path dir = fs::u8path(text);
        if (text.empty() || !fs::exists(dir / "state", ec)) fs::remove(e.path(), ec);
        else out.push_back(dir);
    }
    return out;
}

}
