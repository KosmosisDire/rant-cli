#include <algorithm>
#include <memory>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/enable.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "commands/summary.hpp"
#include "complete/complete.hpp"
#include "run/nodes.hpp"
#include "services/services.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"
#include "util/text.hpp"

namespace commands {

/* Prints a stop's summary, closed by its counts. */
static int done(Summary& summary) {
    summary.closing = summary.counts();
    summary.finish();
    return 0;
}

/* The group names of an instance's roots, for a question before stopping it alone. */
static std::vector<std::string> groups_of(const state::Instance& inst) {
    std::vector<std::string> out;
    for (auto& key : inst.roots)
        if (key.rfind("group ", 0) == 0) out.push_back(key.substr(6));
    return out;
}

/* Drops every node and root of a state but those its services hold, the roots from the nodes
 * left running too. The nodes dropped are stopped first. */
static void keep_only(state::State& st, const Kept& k) {
    st.instances.erase(std::remove_if(st.instances.begin(), st.instances.end(), [&](const state::Instance& i) { return !k.nodes.count(i.name); }),
                       st.instances.end());
    for (auto& i : st.instances)
        i.roots.erase(std::remove_if(i.roots.begin(), i.roots.end(), [&](const std::string& r) { return !k.roots.count(r); }), i.roots.end());
    st.roots.erase(std::remove_if(st.roots.begin(), st.roots.end(), [&](const state::Root& r) { return !k.roots.count(r.key()); }),
                   st.roots.end());
}

/* Stops every node of a state but those its services hold, last started first. */
static void stop_all_but(Summary& summary, state::State& st, const Kept& k) {
    for (auto it = st.instances.rbegin(); it != st.instances.rend(); ++it)
        if (!k.nodes.count(it->name)) summary.add(stopped_row(it->name, run::halt(*it)));
    keep_only(st, k);
}

/* Says which services keep running, when any do. */
static void note_kept(app::Context& ctx, const std::vector<std::string>& running) {
    if (!running.empty()) ctx.out.note("services keep running, name one to stop it: " + util::joined(running));
}

/* Stops everything rant started in this workspace but its services, last started first, once
 * the list has been shown and the question answered. */
static int stop_workspace(app::Context& ctx, const config::Workspace& ws) {
    run::Session s(ws);
    state::State& st = s.state();
    Kept k = kept_services(st, ws.data);
    std::vector<std::string> names, groups;
    for (auto& i : st.instances)
        if (!k.nodes.count(i.name)) names.push_back(i.name);
    for (auto& r : st.roots) {
        if (r.kind != "group" || k.roots.count(r.key())) continue;
        /* params tell runs of one group apart, as ls shows them */
        auto runs = std::count_if(st.roots.begin(), st.roots.end(), [&](const state::Root& o) { return o.kind == "group" && o.name == r.name; });
        groups.push_back(runs > 1 ? r.key().substr(6) : r.name);
    }
    if (names.empty()) {
        keep_only(st, k);
        s.save();
        if (k.running.empty()) ctx.out.line("nothing started here is running");
        else ctx.out.line("only services run here, name one to stop it: " + util::joined(k.running));
        return 0;
    }
    if (!groups.empty()) ctx.out.line("groups: " + util::joined(groups));
    ctx.out.line("nodes:  " + util::joined(names));
    note_kept(ctx, k.running);
    std::string n = std::to_string(names.size());
    if (!ui::confirm("Stop all " + n + (names.size() == 1 ? " node?" : " nodes?"), ctx.yes)) return 1;
    Summary summary(ctx, "");
    stop_all_but(summary, st, k);
    s.save();
    return done(summary);
}

/* One workspace's state, locked until this command ends, and what its services hold. */
struct Held {
    config::fs::path              root;
    std::unique_ptr<state::Store> store;
    Kept                          kept;
    state::State& state() { return store->state(); }
};

/* Every workspace on this machine with nodes rant started, this one first. The rest lock
 * in path order, so two stops never wait on each other. */
static std::vector<Held> started_anywhere(app::Context& ctx) {
    std::vector<config::fs::path> dirs = state::remembered();
    std::sort(dirs.begin(), dirs.end());
    if (const config::Workspace* ws = ctx.scope()) dirs.insert(dirs.begin(), ws->data);
    std::vector<Held> out;
    std::vector<std::string> seen;
    for (auto& d : dirs) {
        std::string key = util::path_key(d);
        if (std::find(seen.begin(), seen.end(), key) != seen.end()) continue;
        seen.push_back(key);
        Held h{ state::folder_of(d), std::make_unique<state::Store>(d), {} };
        state::prune(h.state(), services::running_in);
        h.store->save();
        h.kept = kept_services(h.state(), d);
        if (!h.state().instances.empty()) out.push_back(std::move(h));
    }
    return out;
}

/* A node on this machine that no workspace started, found on the mesh by its address. */
struct Loose {
    std::string name;
    uint64_t    pid = 0;
};

/* The mesh's nodes on this machine that no held workspace started, only those named `only`
 * when given. A node that does not report its process id cannot be stopped and is named. */
static std::vector<Loose> loose_here(app::Context& ctx, mesh::Client& mesh, std::vector<Held>& held,
                                     const std::string& only = "") {
    std::map<uint32_t, std::string> names;
    for (auto& p : mesh.peers())
        if (p.here && (only.empty() || config::same_name(p.name, only))) names[p.id] = p.name;
    std::vector<uint32_t> ids;
    for (auto& [id, name] : names) ids.push_back(id);
    auto pids = mesh.pids(ids);
    std::vector<Loose> out;
    for (auto& [id, name] : names) {
        auto it = pids.find(id);
        if (it == pids.end()) {
            ctx.out.warn("`" + name + "` did not report its pid, cannot stop it");
            continue;
        }
        bool started = false;
        for (auto& h : held)
            for (auto& i : h.state().instances) started = started || process::owns(i.tracking, it->second);
        if (!started) out.push_back({ name, it->second });
    }
    return out;
}

/* Stops every node on this machine but the services: what any workspace started, on any
 * domain, and what runs here without rant on the domain in use. */
static int stop_everything(app::Context& ctx) {
    auto held = started_anywhere(ctx);
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    auto loose = loose_here(ctx, mesh, held);

    std::vector<std::pair<std::string, std::string>> rows;
    std::vector<std::string> kept;
    size_t count = loose.size();
    for (auto& h : held) {
        kept.insert(kept.end(), h.kept.running.begin(), h.kept.running.end());
        std::vector<std::string> names;
        for (auto& i : h.state().instances)
            if (!h.kept.nodes.count(i.name)) names.push_back(i.name);
        if (names.empty()) continue;
        rows.push_back({ ctx.shown(h.root), util::joined(names) });
        count += names.size();
    }
    if (!loose.empty()) {
        std::vector<std::string> names;
        for (auto& l : loose) names.push_back(l.name);
        rows.push_back({ "not started by rant", util::joined(names) });
    }
    if (count == 0) {
        if (kept.empty()) ctx.out.line("no node is running on this machine");
        else ctx.out.line("only services run on this machine, name one to stop it: " + util::joined(kept));
        return 0;
    }
    size_t width = 0;
    for (auto& r : rows) width = std::max(width, r.first.size());
    for (auto& r : rows) ctx.out.line(r.first + ":" + std::string(width - r.first.size() + 2, ' ') + r.second);
    note_kept(ctx, kept);
    if (!ui::confirm("Stop all " + std::to_string(count) + (count == 1 ? " node?" : " nodes?"), ctx.yes)) return 1;

    Summary summary(ctx, "");
    for (auto& h : held) {
        stop_all_but(summary, h.state(), h.kept);
        h.store->save();
    }
    for (auto& l : loose) summary.add(stopped_row(l.name, process::stop_pid(l.pid, run::grace)));
    return done(summary);
}

/* Stops the node of that name this workspace's rant started, asking first when a running group
 * holds it. in names the workspace when it is not the one rant runs in. */
static int stop_here(app::Context& ctx, const std::string& name, const std::string& in) {
    run::Session s(ctx.require_scope());
    state::Instance* inst = s.state().instance(name);
    if (!inst) throw app::Failure("`" + name + "` stopped already");
    std::string node = inst->name;
    auto groups = groups_of(*inst);
    std::string where = in.empty() ? "" : " in " + in;
    if (!groups.empty() && !ui::confirm("`" + node + "` belongs to the running group " + util::joined(groups) + where + ". Stop it anyway?", ctx.yes))
        return 1;
    Summary summary(ctx, "");
    Summary::Row row = stopped_row(node, run::halt(*inst));
    if (!in.empty()) row.detail += (row.detail.empty() ? "" : ", ") + std::string("started in ") + in;
    run::forget(s.state(), node);
    s.save();
    summary.add(row);
    return done(summary);
}

/* Stops a node on this machine no workspace's rant started, found on the mesh, else says
 * where it runs or that it runs nowhere. */
static int stop_loose(app::Context& ctx, const std::string& name) {
    auto held = started_anywhere(ctx);
    Summary summary(ctx, "");
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    auto loose = loose_here(ctx, mesh, held, name);
    for (auto& l : loose) {
        Summary::Row row = stopped_row(l.name, process::stop_pid(l.pid, run::grace));
        row.detail += (row.detail.empty() ? "" : ", ") + std::string("started outside rant");
        summary.add(row);
    }
    if (!loose.empty()) return done(summary);
    for (auto& p : mesh.peers())
        if (config::same_name(p.name, name) && !p.here)
            throw app::Failure("`" + name + "` is running on " + p.host() + ", rant only stops nodes on this machine");
    throw app::Failure("no node `" + name + "` on domain " + std::to_string(ctx.domain()) + ", see `rant ls nodes`");
}

/* Stops one node wherever on this machine rant started it, picking when several workspaces
 * did, else one rant did not start. */
int stop_node(app::Context& ctx, const std::string& name) {
    auto where = where_running(ctx, name, true, "stop");
    if (!where) return 1;
    if (where->empty()) return stop_loose(ctx, name);
    return in_each(ctx, *where, [&](const std::string& in) { return stop_here(ctx, name, in); });
}

/* The group roots to stop: the one root params name, else every root of the group. The
 * group's own name is looked up, so a bare name finds a group in a package. */
static std::vector<state::Root> targets(app::Context& ctx, run::Session& s, const std::string& group,
                                        const std::vector<std::string>& params) {
    if (!params.empty()) {
        config::Plan plan = config::plan_group(ctx.cwd(), group, params);
        require_plan(ctx, plan);
        state::Root root{ "group", plan.group, plan.params };
        if (!s.state().root(root.key())) throw app::Failure("`" + root.key().substr(6) + "` is not running, see `rant ls`");
        return { root };
    }
    std::string name = group_name(ctx, group);
    std::vector<state::Root> out;
    for (auto& r : s.state().roots)
        if (r.kind == "group" && r.name == name) out.push_back(r);
    if (out.empty()) throw app::Failure("group `" + name + "` is not running, see `rant ls`");
    return out;
}

/* Stops the nodes a group's own blocks made inside the runs of the groups that include it,
 * last started first, whichever of them hold the nodes. */
static int stop_inside(app::Context& ctx, run::Session& s, const std::string& group, const std::vector<std::string>& names) {
    Summary summary(ctx, group);
    state::State& st = s.state();
    for (auto it = names.rbegin(); it != names.rend(); ++it) {
        const state::Instance* inst = st.instance(*it);
        Summary::Row row = stopped_row(*it, run::halt(*inst));
        if (row.detail.empty()) row.detail = held_by(inst->roots, "inside");
        run::forget(st, *it);
        summary.add(row);
    }
    s.save();
    return done(summary);
}

/* Stops what the group started. A node of the same name it did not start keeps running. A
 * group that runs only inside others stops its own nodes there. */
int stop_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params) {
    run::Session s(ctx.require_scope());
    if (params.empty()) {
        std::string name = group_name(ctx, group);
        bool own = std::any_of(s.state().roots.begin(), s.state().roots.end(), [&](const state::Root& r) { return r.kind == "group" && r.name == name; });
        auto inside = run::made_by(s.state(), name);
        if (!own && !inside.empty()) return stop_inside(ctx, s, name, inside);
    }
    auto roots = targets(ctx, s, group, params);
    if (roots.empty()) return 1;
    std::string shown;
    for (auto& [k, v] : roots.front().params) shown += (shown.empty() ? "" : " ") + k + "=" + v;
    Summary summary(ctx, roots.front().name, roots.size() == 1 ? shown : "");
    for (auto& r : roots) {
        for (auto& rel : run::release_root(s, r.key())) {
            if (rel.kept_for.empty()) {
                summary.add(stopped_row(rel.name, rel.how));
                continue;
            }
            summary.add({ rel.name, Summary::Mark::Running, "kept running", held_by(rel.kept_for, "still needed by") });
        }
        s.save();
    }
    return done(summary);
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (ctx.args.has("all")) {
        if (!w.empty()) throw app::UsageError("--all takes no group or node");
        return stop_everything(ctx);
    }
    if (w.empty()) {
        const config::Workspace* ws = ctx.workspace();
        return ws ? stop_workspace(ctx, *ws) : stop_everything(ctx);
    }
    if (w[0] == "node") {
        if (w.size() != 2) throw app::UsageError("stop node takes one node, such as `rant stop node talker`");
        return stop_node(ctx, w[1]);
    }
    const std::string name = w[0];
    std::vector<std::string> params(w.begin() + 1, w.end());
    auto where = where_running(ctx, name, false, "stop");
    if (!where) return 1;
    if (where->empty()) {
        /* it runs nowhere rant started it: a group says so, a node may run without rant */
        if (names_group(ctx, name)) return stop_group(ctx, name, params);
        if (!params.empty()) throw app::UsageError("a node takes no key=value, only a group does");
        return stop_loose(ctx, name);
    }
    params = resolved_params(ctx, name, params);
    return in_each(ctx, *where, [&](const std::string& in) {
        if (names_group(ctx, name)) {
            if (!in.empty()) ctx.out.note("`" + name + "` runs in " + in);
            return stop_group(ctx, name, params);
        }
        if (!params.empty()) throw app::UsageError("a node takes no key=value, only a group does");
        return stop_here(ctx, name, in);
    });
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.running_groups();
        for (auto& n : r.running_nodes()) out.push_back(n);
        out.push_back("node");
        return { out };
    }
    if (w[0] == "node") return { w.size() == 1 ? r.running_nodes() : std::vector<std::string>{} };
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command stop() {
    app::Command c{ "stop", "[<group> [key=value...] | [node] <node>]",
                    "stop any node on this machine, all this workspace started when nothing is named, a service only by name",
                    app::Section::Workspace,
                    { { "all", 'a', "", "stop every node on this machine but the services, whoever started it" } }, run };
    c.complete = complete_words;
    return c;
}

}
