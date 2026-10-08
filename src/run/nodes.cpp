#include "run/nodes.hpp"

#include <algorithm>
#include <cctype>
#include <cstdio>

namespace run {

Session::Session(const config::Workspace& ws) : ws_(ws), store_(ws.data) {
    state::prune(store_.state());
}

state::State snapshot(app::Context& ctx) {
    const config::Workspace* ws = ctx.workspace();
    if (!ws) return {};
    Session s(*ws);
    s.save();
    return s.state();
}

/* A job name unique per workspace and node: the same name in another workspace is another
 * job. FNV-1a of the root path, which never changes for a workspace. */
static std::string job_name(const config::fs::path& root, const std::string& name) {
    std::string key = config::to_utf8(root);
#ifdef _WIN32
    for (auto& c : key) c = (char)std::tolower((unsigned char)c);
#endif
    uint64_t h = 1469598103934665603ull;
    for (unsigned char c : key) h = (h ^ c) * 1099511628211ull;
    char hex[17];
    std::snprintf(hex, sizeof hex, "%016llx", (unsigned long long)h);
    std::string safe = name;
    for (auto& c : safe)
        if (c == '\\') c = '_';
    return std::string("Local\\rant-") + std::string(hex, 12) + "-" + safe;
}

state::Instance spawn(const config::Workspace& ws, const config::Instance& inst, const std::string& name) {
    /* a prefixed name like left/lidar logs to a folder of its prefix */
    auto log = ws.logs / config::from_utf8(name + ".log");
    config::fs::create_directories(log.parent_path());
    std::error_code ec;
    if (config::fs::exists(log, ec)) {
        auto previous = log;
        previous += ".1";
        config::fs::rename(log, previous, ec);
    }

    process::Command c{ inst.argv, inst.cwd, inst.env };
    /* Launcher variables: the node's mesh name, read by the Rant library, and unbuffered
     * Python so its log keeps up. The name holds its node prefix already. */
    c.env["RANT_NODE_NAME"] = name;
    c.env["RANT_NODE_NAME_PREFIX"] = "";
    c.env["PYTHONUNBUFFERED"] = "1";

    state::Instance rec;
    rec.name = name;
    rec.type = inst.type;
    rec.argv = inst.argv;
    rec.env = inst.env;
    rec.cwd = config::to_utf8(inst.cwd);
    rec.log = config::to_utf8(log);
    rec.tracking = process::start_detached(c, log, job_name(ws.root, name));
    return rec;
}

process::Stopped halt(const state::Instance& inst) {
    return process::stop(inst.tracking, grace);
}

bool same_spec(const state::Instance& running, const config::Instance& planned) {
    return running.type == planned.type && running.argv == planned.argv && running.env == planned.env &&
           running.cwd == config::to_utf8(planned.cwd);
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
        if (!inst.roots.empty()) {
            out.push_back({ inst.name, inst.roots, process::Stopped::AlreadyGone });
            continue;
        }
        Released r{ inst.name, {}, halt(inst) };
        st.instances.erase(st.instances.begin() + (long)i);
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
