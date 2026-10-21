#include "services/services.hpp"

#include <algorithm>
#include <chrono>
#include <ctime>
#include <fstream>
#include <list>
#include <optional>
#include <string>
#include <thread>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "process/supervisor.hpp"
#include "run/nodes.hpp"
#include "state/state.hpp"
#include "util/home.hpp"
#include "util/lock.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <fcntl.h>
#include <poll.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace services {

using clock = std::chrono::steady_clock;
using std::chrono::seconds;

fs::path log_file() { return util::rant_home() / "logs" / "services.log"; }

/* Tells this home's service process from another home's, as a test's. */
static std::string home_key() { return util::path_key(util::rant_home()).substr(0, 12); }

#ifdef _WIN32

uint32_t session() {
    DWORD id = 0;
    ProcessIdToSessionId(GetCurrentProcessId(), &id);
    return id;
}

static fs::path lock_file(uint32_t s) { return util::rant_home() / ("services-" + std::to_string(s) + ".lock"); }

/* Global, so a command in another session reaches it. */
static std::wstring wake_event(uint32_t s) {
    return config::from_utf8("Global\\rant-services-" + home_key() + "-" + std::to_string(s)).wstring();
}

void wake(uint32_t s) {
    if (HANDLE e = OpenEventW(EVENT_MODIFY_STATE, FALSE, wake_event(s).c_str())) {
        SetEvent(e);
        CloseHandle(e);
    }
}

#else

uint32_t session() { return 0; }

static fs::path lock_file(uint32_t) { return util::rant_home() / "services.lock"; }

static fs::path wake_pipe() { return util::rant_home() / "services.wake"; }

/* Opening fails with no reader, so only a running service process hears it. */
void wake(uint32_t) {
    int fd = open(wake_pipe().c_str(), O_WRONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return;
    char c = 1;
    ssize_t n = write(fd, &c, 1);
    (void)n;
    close(fd);
}

#endif

bool running_in(uint32_t s) {
    std::error_code ec;
    fs::create_directories(util::rant_home(), ec);
    return !util::FileLock(lock_file(s), false).held();
}

bool running() { return running_in(session()); }

/* A stop request is a file named after the session and the node, answered by a file saying how
 * it stopped. */
static fs::path stops() { return util::rant_home() / "stops"; }

std::optional<process::Stopped> stop_through(uint32_t s, const process::Tracking& t) {
    if (!running_in(s)) return std::nullopt;
    std::error_code ec;
    fs::create_directories(stops(), ec);
    std::string base = std::to_string(s) + "-" + std::to_string(t.pid) + "-" + std::to_string(t.start_time);
    fs::path ask = stops() / (base + ".ask"), done = stops() / (base + ".done");
    std::ofstream(ask, std::ios::binary) << t.pid << " " << t.start_time << " " << t.job << "\n";
    wake(s);
    for (int i = 0; i < 150; i++) {
        std::string how;
        if (std::ifstream(done, std::ios::binary) >> how) {
            fs::remove(done, ec);
            return how == "gracefully" ? process::Stopped::Gracefully
                 : how == "killed"     ? process::Stopped::Killed
                                       : process::Stopped::AlreadyGone;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    fs::remove(ask, ec);
    return std::nullopt;
}

void ensure_running() {
    if (running()) return;
    std::error_code ec;
    fs::create_directories(log_file().parent_path(), ec);
    std::string job = "Local\\rant-services-" + home_key() + "-" + std::to_string(clock::now().time_since_epoch().count());
    process::start_detached({ { config::to_utf8(process::self_path()), "__services" }, util::rant_home(), {} }, log_file(), job);
    for (int i = 0; i < 200; i++) {
        if (running()) return;
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    throw app::Failure("rant's service process did not start, see " + config::to_utf8(log_file()));
}

using json = nlohmann::ordered_json;

static fs::path list_file() { return util::rant_home() / "services.json"; }

std::string Enabled::shown() const {
    std::string s = kind == "node" ? "node " + name : name;
    for (auto& w : words) s += " " + w;
    return s;
}

bool Enabled::same(const Enabled& o) const {
    return util::path_key(folder) == util::path_key(o.folder) && kind == o.kind && name == o.name && words == o.words;
}

std::vector<Enabled> enabled() {
    std::ifstream in(list_file(), std::ios::binary);
    if (!in) return {};
    std::vector<Enabled> out;
    try {
        json doc = json::parse(in);
        for (auto& j : doc.at("enabled")) {
            Enabled e;
            e.folder = config::from_utf8(j.at("folder").get<std::string>());
            e.start = j.value("start", "boot");
            if (e.start != "boot" && e.start != "login") throw std::runtime_error("start");
            e.kind = j.at("kind");
            e.name = j.at("name");
            e.words = j.value("words", std::vector<std::string>{});
            if (j.contains("domain")) e.domain = j.at("domain").get<uint16_t>();
            e.env = j.value("env", std::map<std::string, std::string>{});
            out.push_back(std::move(e));
        }
    } catch (const std::exception&) {
        throw app::Failure(config::to_utf8(list_file()) + " is damaged, delete it and enable again");
    }
    return out;
}

void save(const std::vector<Enabled>& list) {
    json all = json::array();
    for (auto& e : list) {
        json j = { { "folder", config::to_utf8(e.folder) }, { "start", e.start }, { "kind", e.kind }, { "name", e.name }, { "words", e.words } };
        if (e.domain) j["domain"] = *e.domain;
        if (!e.env.empty()) j["env"] = e.env;
        all.push_back(j);
    }
    fs::path tmp = list_file();
    tmp += ".tmp";
    {
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        f << json{ { "enabled", all } }.dump(2) << '\n';
        if (!f) throw app::Failure("cannot write " + config::to_utf8(tmp));
    }
    std::error_code ec;
    fs::rename(tmp, list_file(), ec);
    if (ec) throw app::Failure("cannot write " + config::to_utf8(list_file()) + ": " + ec.message());
}

/* One line of the log, after the local time. */
static void say(const std::string& line) {
    std::time_t now = std::time(nullptr);
    char stamp[32] = "";
    if (std::tm* t = std::localtime(&now)) std::strftime(stamp, sizeof stamp, "%Y-%m-%d %H:%M:%S", t);
    std::ofstream(log_file(), std::ios::app | std::ios::binary) << stamp << " " << line << "\n";
}

static std::string own_pid() {
#ifdef _WIN32
    return std::to_string(GetCurrentProcessId());
#else
    return std::to_string(getpid());
#endif
}

static bool same(const process::Tracking& a, const process::Tracking& b) {
    return a.pid == b.pid && a.start_time == b.start_time;
}

/* How long a node that keeps failing soon after it starts waits before the next start. */
static seconds pause(int quick) { return seconds(std::min(60, 1 << std::min(std::max(quick - 1, 0), 6))); }

/* A node is quick to fail when it ends this soon after starting. */
constexpr seconds settled{ 10 };

/* Runs each group or node enabled to start then with rant run in its folder, detached, its
 * output in the log. They hand their nodes with restart set to the service process, so none
 * waits for them. */
static void start_enabled(const std::string& start) {
    std::vector<Enabled> list;
    try {
        list = enabled();
    } catch (const app::Failure& e) {
        say(e.what());
    }
    for (auto& e : list) {
        if (e.start != start) continue;
        if (!fs::is_directory(e.folder)) {
            say("cannot start " + e.shown() + ", " + config::to_utf8(e.folder) + " is gone");
            continue;
        }
        std::vector<std::string> argv = { config::to_utf8(process::self_path()), "run" };
        if (e.kind == "node") argv.insert(argv.end(), { "node", e.name });
        else argv.push_back(e.name);
        argv.insert(argv.end(), e.words.begin(), e.words.end());
        argv.insert(argv.end(), { "--no-build", "-y" });
        if (e.domain) argv.insert(argv.end(), { "--domain", std::to_string(*e.domain) });
        std::string job = "Local\\rant-services-" + home_key() + "-start-" + std::to_string(clock::now().time_since_epoch().count());
        say("at " + start + ", starting " + e.shown() + " in " + config::to_utf8(e.folder));
        try {
            process::start_detached({ argv, e.folder, e.env }, log_file(), job);
        } catch (const app::Failure& f) {
            say("cannot start " + e.shown() + ": " + f.what());
        }
    }
}

class Host {
public:
    /* One round: notes what ended, starts what is due, and looks at every workspace when
     * asked to or now and then. False once it has had nothing to do for a while. */
    bool round(bool woken);

private:
    /* A node this process watches: one it started, or one a former service process
     * started, whose exit code it cannot learn. */
    struct Watched {
        fs::path                      data;
        std::string                   name;
        process::Tracking             tracking;
        std::optional<process::Child> child;
        clock::time_point             since;
        int                           quick = 0;    /* quick failures in a row before this start */
    };

    /* A node that ended and waits for its workspace's lock, or for its time to start again. */
    struct Ended {
        fs::path                         data;
        std::string                      name;
        process::Tracking                tracking;
        int                              code = -1;    /* -1 when not known */
        int                              quick = 0;
        std::optional<clock::time_point> again;
        clock::time_point                checked{};    /* when its record was last read while it waits */
    };

    void note_ended();
    void decide();
    void look();
    void answer_stops();
    bool watching(const fs::path& data, const state::Instance& rec) const;

    std::list<Watched> watched_;
    std::list<Ended>   ended_;
    clock::time_point  looked_{};
    bool               look_again_ = true;
    size_t             live_here_ = 0;    /* nodes of this session alive at the last look */
    clock::time_point  idle_since_ = clock::now();
};

void Host::note_ended() {
    for (auto it = watched_.begin(); it != watched_.end();) {
        std::optional<int> code;
        if (it->child) code = it->child->ended();
        else if (!process::alive(it->tracking)) code = -1;
        if (!code) {
            ++it;
            continue;
        }
        int quick = clock::now() - it->since < settled ? it->quick + 1 : 0;
        ended_.push_back({ it->data, it->name, it->tracking, *code, quick, std::nullopt });
        it = watched_.erase(it);
    }
}

/* A node a command stopped or started again has another record or none, so it is left be. */
void Host::decide() {
    for (auto it = ended_.begin(); it != ended_.end();) {
        /* while it waits its record is read now and then, so a stop takes effect soon */
        bool due = !it->again || clock::now() >= *it->again;
        if (!due && clock::now() - it->checked < seconds(1)) {
            ++it;
            continue;
        }
        state::Store store(it->data, false);
        if (!store.locked()) {
            ++it;
            continue;
        }
        state::Instance* rec = store.state().instance(it->name);
        if (!rec || !same(rec->tracking, it->tracking)) {
            it = ended_.erase(it);
            continue;
        }
        if (!due) {
            it->checked = clock::now();
            ++it;
            continue;
        }
        std::string code = it->code < 0 ? "an unknown code" : "code " + std::to_string(it->code);
        if (!it->again) {
            if (rec->restart != "always" && !(rec->restart == "on-failure" && it->code != 0)) {
                say(it->name + " exited with " + code + ", and its restart is " + rec->restart);
                run::forget(store.state(), it->name);
                store.save();
                it = ended_.erase(it);
                continue;
            }
            seconds wait = pause(it->quick);
            say(it->name + " exited with " + code + ", starting it again in " + std::to_string(wait.count()) + " s");
            it->again = clock::now() + wait;
            ++it;
            continue;
        }
        try {
            process::Child child = run::start(it->data, *rec);
            rec->tracking = child.tracking();
            rec->restarts++;
            store.save();
            say("started " + it->name + " again, restart " + std::to_string(rec->restarts));
            watched_.push_back({ it->data, it->name, child.tracking(), std::move(child), clock::now(), it->quick });
            it = ended_.erase(it);
        } catch (const app::Failure& e) {
            say("cannot start " + it->name + " again: " + e.what());
            it->again = clock::now() + pause(it->quick + 1);
            it->quick++;
            ++it;
        }
    }
}

bool Host::watching(const fs::path& data, const state::Instance& rec) const {
    auto mine = [&](const fs::path& d, const std::string& n) { return d == data && n == rec.name; };
    for (auto& w : watched_)
        if (mine(w.data, w.name) && same(w.tracking, rec.tracking)) return true;
    for (auto& e : ended_)
        if (mine(e.data, e.name)) return true;
    return false;
}

/* Starts the nodes of this session commands left to it, and takes on those with restart set a
 * former service process started. A workspace whose state is busy is looked at again next round. */
void Host::look() {
    look_again_ = false;
    live_here_ = 0;
    for (auto& data : state::remembered()) {
        state::Store store(data, false);
        if (!store.locked()) {
            look_again_ = true;
            continue;
        }
        bool changed = false;
        auto& instances = store.state().instances;
        for (size_t i = 0; i < instances.size(); i++) {
            state::Instance& rec = instances[i];
            if (rec.session != session()) continue;
            bool alive = rec.tracking.pid && process::alive(rec.tracking);
            live_here_ += alive;
            if (watching(data, rec)) continue;
            if (rec.tracking.pid == 0) {
                try {
                    process::Child child = run::start(data, rec);
                    rec.tracking = child.tracking();
                    watched_.push_back({ data, rec.name, child.tracking(), std::move(child), clock::now(), 0 });
                    say("started " + rec.name + " in " + config::to_utf8(state::folder_of(data)) + ", restart " + rec.restart);
                } catch (const app::Failure& e) {
                    std::string name = rec.name;
                    say("cannot start " + name + ": " + e.what());
                    run::forget(store.state(), name);
                    i--;    /* the next record took its place */
                }
                changed = true;
            } else if (rec.restart == "never") {
                continue;
            } else if (alive) {
                watched_.push_back({ data, rec.name, rec.tracking, std::nullopt, clock::now(), 0 });
                say("watching " + rec.name + ", which an earlier service process started");
            } else {
                ended_.push_back({ data, rec.name, rec.tracking, -1, 0, std::nullopt });
            }
        }
        if (changed) store.save();
    }
}

/* Stops the nodes of this session commands of another session asked it to stop. */
void Host::answer_stops() {
    std::error_code ec;
    std::string mine = std::to_string(session()) + "-";
    for (auto& f : fs::directory_iterator(stops(), ec)) {
        std::string name = f.path().filename().u8string();
        if (name.rfind(mine, 0) != 0 || f.path().extension() != ".ask") continue;
        process::Tracking t;
        std::ifstream in(f.path(), std::ios::binary);
        in >> t.pid >> t.start_time;
        std::getline(in >> std::ws, t.job);
        in.close();
        fs::remove(f.path(), ec);
        process::Stopped how = process::stop(t, run::grace);
        const char* word = how == process::Stopped::Gracefully ? "gracefully" : how == process::Stopped::AlreadyGone ? "gone" : "killed";
        fs::path done = f.path();
        std::ofstream(done.replace_extension(".done"), std::ios::binary) << word << "\n";
    }
}

bool Host::round(bool woken) {
    answer_stops();
    note_ended();
    decide();
    if (woken || look_again_ || clock::now() - looked_ > seconds(5)) {
        look();
        looked_ = clock::now();
    }
#ifdef _WIN32
    /* session 0, where the boot task runs, has no desktop or terminal, so only this process can
     * stop its nodes gently */
    bool keep = session() == 0 && live_here_ > 0;
#else
    bool keep = false;
#endif
    if (keep || !watched_.empty() || !ended_.empty() || look_again_) idle_since_ = clock::now();
    return clock::now() - idle_since_ < seconds(10);
}

int serve(const std::string& start) {
    std::error_code ec;
    fs::create_directories(log_file().parent_path(), ec);
    /* An entry only starts what is enabled and a service process to watch it, then ends, so
     * the console window older Windows gives it closes and no service manager holds the nodes. */
    if (!start.empty()) {
        start_enabled(start);
        try {
            ensure_running();
        } catch (const app::Failure& e) {
            say(e.what());
            return 1;
        }
        return 0;
    }
    /* a command probing whether one runs holds the lock for a moment, so a few tries */
    std::optional<util::FileLock> lock;
    for (int i = 0; i < 20; i++) {
        lock.emplace(lock_file(session()), false);
        if (lock->held()) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    if (!lock->held()) return 0;    /* another one runs */

#ifdef _WIN32
    SECURITY_ATTRIBUTES sa{ sizeof sa, process::user_only_security(), FALSE };
    HANDLE event = CreateEventW(&sa, FALSE, FALSE, wake_event(session()).c_str());
    auto wait = [&] { return WaitForSingleObject(event, 200) == WAIT_OBJECT_0; };
#else
    /* opened for writing too, so the pipe never reads as closed while no command writes */
    mkfifo(wake_pipe().c_str(), 0600);
    int pipe = open(wake_pipe().c_str(), O_RDWR | O_NONBLOCK | O_CLOEXEC);
    auto wait = [&] {
        pollfd p{ pipe, POLLIN, 0 };
        if (pipe < 0 || poll(&p, 1, 200) <= 0) return false;
        char buf[64];
        while (read(pipe, buf, sizeof buf) > 0) {}
        return true;
    };
#endif
    say("service process " + own_pid() + " started");
    Host host;
    bool woken = true;
    while (host.round(woken)) woken = wait();
    say("service process " + own_pid() + " stopped, nothing to watch");
#ifdef _WIN32
    CloseHandle(event);
#else
    if (pipe >= 0) close(pipe);
#endif
    return 0;
}

}
