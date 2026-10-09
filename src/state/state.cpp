#include "state/state.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "util/home.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

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
                              { "roots", i.roots } });
    return { { "format", FORMAT }, { "roots", roots }, { "instances", instances } };
}

Store::Store(const fs::path& data_dir) : path_(data_dir / "state") {
    fs::create_directories(data_dir);
    fs::path lock = data_dir / "state.lock";
#ifdef _WIN32
    /* Opened without inheritance, so no node ever holds the lock. */
    HANDLE h = CreateFileW(lock.wstring().c_str(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                           nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) throw app::Failure("cannot open " + lock.u8string());
    OVERLAPPED ov{};
    if (!LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, &ov)) {
        CloseHandle(h);
        throw app::Failure("cannot lock " + lock.u8string());
    }
    lock_ = h;
#else
    /* Close on exec, so no node ever holds the lock. */
    int fd = open(lock.c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0644);
    if (fd < 0) throw app::Failure("cannot open " + lock.string() + ": " + std::strerror(errno));
    while (flock(fd, LOCK_EX) != 0) {
        if (errno == EINTR) continue;
        close(fd);
        throw app::Failure("cannot lock " + lock.string() + ": " + std::strerror(errno));
    }
    lock_ = fd;
#endif
    std::ifstream f(path_, std::ios::binary);
    if (!f) return;
    try {
        state_ = from_json(json::parse(f));
    } catch (const std::exception&) {
        throw app::Failure(path_.u8string() + " is damaged, delete it to start over (running nodes are then forgotten)");
    }
}

Store::~Store() {
#ifdef _WIN32
    if (lock_) {
        OVERLAPPED ov{};
        UnlockFileEx(lock_, 0, 1, 0, &ov);
        CloseHandle(lock_);
    }
#else
    if (lock_ >= 0) close(lock_);    /* closing drops the flock */
#endif
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

std::vector<std::string> prune(State& s) {
    std::vector<std::string> dropped;
    for (auto it = s.instances.begin(); it != s.instances.end();) {
        if (process::alive(it->tracking)) {
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
