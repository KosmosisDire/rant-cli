#include "run/nodes.hpp"

#include <algorithm>
#include <thread>

#include "app/failure.hpp"
#include "services/services.hpp"
#include "util/home.hpp"

namespace run {

Session::Session(const config::Workspace& ws) : ws_(ws), store_(ws.data) {
    state::prune(store_.state(), services::running());
}

state::State snapshot(app::Context& ctx) {
    const config::Workspace* ws = ctx.scope();
    if (!ws) return {};
    Session s(*ws);
    s.save();
    return s.state();
}

/* A job name unique per workspace and node: the same name in another workspace is another
 * job. The data folder never changes for a workspace. */
static std::string job_name(const config::fs::path& data, const std::string& name) {
    std::string safe = name;
    for (auto& c : safe)
        if (c == '\\') c = '_';
    return "Local\\rant-" + util::path_key(data).substr(0, 12) + "-" + safe;
}

std::vector<Exit> exited_within(const std::vector<state::Instance>& started, std::chrono::milliseconds span) {
    auto begin = std::chrono::steady_clock::now(), end = begin + span;
    std::vector<Exit> out;
    for (;;) {
        auto now = std::chrono::steady_clock::now();
        for (auto& i : started)
            if (std::none_of(out.begin(), out.end(), [&](const Exit& e) { return e.instance == &i; }) && !process::alive(i.tracking))
                out.push_back({ &i, std::chrono::duration_cast<std::chrono::milliseconds>(now - begin) });
        if (out.size() == started.size() || now >= end) return out;
        std::this_thread::sleep_for(std::min<std::chrono::steady_clock::duration>(std::chrono::milliseconds(100), end - now));
    }
}

process::Child start(const config::fs::path& data, const state::Instance& rec) {
    auto log = config::from_utf8(rec.log);
    config::fs::create_directories(log.parent_path());
    std::error_code ec;
    if (config::fs::exists(log, ec)) {
        auto previous = log;
        previous += ".1";
        config::fs::rename(log, previous, ec);
    }

    process::Command c{ rec.argv, config::from_utf8(rec.cwd), rec.env, {}, std::nullopt };
    if (!rec.launch_env.empty()) c.base = rec.launch_env;
    /* Launcher variables: the node's mesh name, read by the Rant library, and unbuffered
     * Python so its log keeps up. The name holds its node prefix already. */
    c.env["RANT_NODE_NAME"] = rec.name;
    c.env["RANT_NODE_NAME_PREFIX"] = "";
    c.env["PYTHONUNBUFFERED"] = "1";
    return process::start_child(c, log, job_name(data, rec.name));
}

state::Instance spawn(const config::Workspace& ws, const config::Instance& inst, const std::string& name) {
    state::Instance rec;
    rec.name = name;
    rec.type = inst.type;
    rec.argv = inst.argv;
    rec.env = inst.env;
    rec.cwd = config::to_utf8(inst.cwd);
    /* a prefixed name like left/lidar logs to a folder of its prefix */
    rec.log = config::to_utf8(ws.logs / config::from_utf8(name + ".log"));
    rec.group = inst.group;
    rec.expect_mesh = inst.expect_mesh;
    rec.restart = inst.restart;
    state::remember(ws.data);
    /* running first, so no command drops the record as dead before it starts the node */
    if (rec.restart != "never") {
        services::ensure_running();
        rec.launch_env = process::environment({});
    } else {
        rec.tracking = start(ws.data, rec).tracking();
    }
    return rec;
}

std::vector<state::Instance> hand_over(const config::fs::path& data, std::vector<state::Instance> started) {
    auto waiting = [&] { return std::any_of(started.begin(), started.end(), [](const state::Instance& i) { return i.tracking.pid == 0; }); };
    if (!waiting()) return started;
    state::remember(data);    /* again, since the state file it needs now exists */
    services::wake();
    for (int i = 0; waiting(); i++) {
        if (i == 300) throw app::Failure("rant's service process did not start the nodes in 15 s, see " + config::to_utf8(services::log_file()));
        if (i > 0) std::this_thread::sleep_for(std::chrono::milliseconds(50));
        if (i % 20 == 19 && !services::running()) {
            services::ensure_running();    /* it ended just before the records came */
            services::wake();
        }
        state::Store store(data);
        for (auto& rec : started) {
            if (rec.tracking.pid) continue;
            const state::Instance* now = store.state().instance(rec.name);
            if (!now) throw app::Failure("rant's service process could not start `" + rec.name + "`, see " + config::to_utf8(services::log_file()));
            if (now->tracking.pid) rec = *now;
        }
    }
    return started;
}

std::vector<Paused> pause_from(const config::fs::path& folder) {
    std::vector<Paused> out;
    for (auto& data : state::remembered()) {
        state::Store store(data);
        state::State& st = store.state();
        state::prune(st, services::running());
        std::vector<std::string> names;
        for (auto it = st.instances.rbegin(); it != st.instances.rend(); ++it) {
            if (!it->runs_from(folder)) continue;
            halt(*it);
            out.push_back({ data, *it });
            names.push_back(it->name);
        }
        for (auto& name : names) forget(st, name);
        store.save();
    }
    return out;
}

std::vector<std::string> resume(const std::vector<Paused>& paused) {
    std::vector<std::string> failed;
    std::vector<config::fs::path> folders;
    for (auto& p : paused)
        if (std::find(folders.begin(), folders.end(), p.data) == folders.end()) folders.push_back(p.data);
    for (auto& data : folders) {
        std::vector<state::Instance> started;
        {
            state::Store store(data);
            state::State& st = store.state();
            for (auto it = paused.rbegin(); it != paused.rend(); ++it) {
                if (it->data != data || st.instance(it->rec.name)) continue;
                state::Instance rec = it->rec;
                rec.tracking = {};
                try {
                    if (rec.restart != "never") services::ensure_running();
                    else rec.tracking = start(data, rec).tracking();
                } catch (const app::Failure& e) {
                    failed.push_back(rec.name + ": " + e.what());
                    continue;
                }
                for (auto& key : rec.roots)
                    if (key == "node " + rec.name && !st.root(key)) st.roots.push_back({ "node", rec.name, {} });
                st.instances.push_back(rec);
                started.push_back(rec);
            }
            store.save();
        }
        try {
            hand_over(data, started);
        } catch (const app::Failure& e) {
            failed.push_back(e.what());
        }
    }
    return failed;
}

process::Stopped halt(const state::Instance& inst) {
    return process::stop(inst.tracking, grace);
}

bool same_spec(const state::Instance& running, const config::Instance& planned) {
    return running.type == planned.type && running.argv == planned.argv && running.env == planned.env &&
           running.cwd == config::to_utf8(planned.cwd);
}

std::vector<std::string> made_by(const state::State& st, const std::string& group) {
    std::vector<std::string> out;
    for (auto& i : st.instances)
        if (i.group == group) out.push_back(i.name);
    return out;
}

bool held_by_group(const state::Instance& inst) {
    return std::any_of(inst.roots.begin(), inst.roots.end(), [](const std::string& k) { return k.rfind("group ", 0) == 0; });
}

void forget(state::State& st, const std::string& name) {
    st.instances.erase(std::remove_if(st.instances.begin(), st.instances.end(),
                                      [&](const state::Instance& i) { return i.name == name; }),
                       st.instances.end());
    st.roots.erase(std::remove_if(st.roots.begin(), st.roots.end(),
                                  [&](const state::Root& r) { return r.kind == "node" && r.name == name; }),
                   st.roots.end());
}

std::vector<Released> release_root(Session& s, const std::string& key) {
    state::State& st = s.state();
    st.roots.erase(std::remove_if(st.roots.begin(), st.roots.end(), [&](const state::Root& r) { return r.key() == key; }),
                   st.roots.end());
    std::vector<Released> out;
    for (size_t i = st.instances.size(); i-- > 0;) {
        state::Instance& inst = st.instances[i];
        auto it = std::find(inst.roots.begin(), inst.roots.end(), key);
        if (it == inst.roots.end()) continue;
        inst.roots.erase(it);
        if (held_by_group(inst)) {
            out.push_back({ inst.name, inst.roots, process::Stopped::AlreadyGone });
            continue;
        }
        Released r{ inst.name, {}, halt(inst) };
        forget(st, r.name);
        out.push_back(r);
    }
    return out;
}

std::map<uint32_t, std::string> managed_peers(mesh::Client& mesh, const state::State& st) {
    std::map<uint32_t, std::string> out;
    if (st.instances.empty()) return out;
    std::vector<uint32_t> ids;
    for (auto& p : mesh.peers()) ids.push_back(p.id);
    for (auto& [peer, pid] : mesh.pids(ids))
        for (auto& inst : st.instances)
            if (process::owns(inst.tracking, pid)) {
                out[peer] = inst.name;
                break;
            }
    return out;
}

}
