#include <algorithm>
#include <memory>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "commands/summary.hpp"
#include "complete/complete.hpp"
#include "run/nodes.hpp"
#include "services/services.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace commands {

static std::string list(const std::vector<std::string>& v) {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : ", ") + x;
    return s;
}

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

/* Stops everything rant started in this workspace, last started first, once the list has
 * been shown and the question answered. */
static int stop_workspace(app::Context& ctx, const config::Workspace& ws) {
    run::Session s(ws);
    state::State& st = s.state();
    if (st.instances.empty()) {
        st.roots.clear();
        s.save();
        ctx.out.line("nothing started here is running");
        return 0;
    }
    std::vector<std::string> names, groups;
    for (auto& i : st.instances) names.push_back(i.name);
    for (auto& r : st.roots) {
        if (r.kind != "group") continue;
        /* params tell runs of one group apart, as ls shows them */
        auto runs = std::count_if(st.roots.begin(), st.roots.end(), [&](const state::Root& o) { return o.kind == "group" && o.name == r.name; });
        groups.push_back(runs > 1 ? r.key().substr(6) : r.name);
    }
    if (!groups.empty()) ctx.out.line("groups: " + list(groups));
    ctx.out.line("nodes:  " + list(names));
    std::string n = std::to_string(names.size());
    if (!ui::confirm("Stop all " + n + (names.size() == 1 ? " node?" : " nodes?"), ctx.yes)) return 1;
    Summary summary(ctx, "");
    for (auto it = st.instances.rbegin(); it != st.instances.rend(); ++it) summary.add(stopped_row(it->name, run::halt(*it)));
    st.instances.clear();
    st.roots.clear();
    s.save();
    return done(summary);
}

/* One workspace's state, locked until this command ends. */
struct Held {
    config::fs::path              root;
    std::unique_ptr<state::Store> store;
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
        Held h{ state::folder_of(d), std::make_unique<state::Store>(d) };
        state::prune(h.state(), services::running_in);
        h.store->save();
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
        if (p.here && (only.empty() || p.name == only)) names[p.id] = p.name;
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

/* Stops every node on this machine: what any workspace started, on any domain, and what
 * runs here without rant on the domain in use. */
static int stop_everything(app::Context& ctx) {
    auto held = started_anywhere(ctx);
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    auto loose = loose_here(ctx, mesh, held);

    std::vector<std::pair<std::string, std::string>> rows;
    size_t count = loose.size();
    for (auto& h : held) {
        std::vector<std::string> names;
        for (auto& i : h.state().instances) names.push_back(i.name);
        rows.push_back({ ctx.shown(h.root), list(names) });
        count += names.size();
    }
    if (!loose.empty()) {
        std::vector<std::string> names;
        for (auto& l : loose) names.push_back(l.name);
        rows.push_back({ "not started by rant", list(names) });
    }
    if (count == 0) {
        ctx.out.line("no node is running on this machine");
        return 0;
    }
    size_t width = 0;
    for (auto& r : rows) width = std::max(width, r.first.size());
    for (auto& r : rows) ctx.out.line(r.first + ":" + std::string(width - r.first.size() + 2, ' ') + r.second);
    if (!ui::confirm("Stop all " + std::to_string(count) + (count == 1 ? " node?" : " nodes?"), ctx.yes)) return 1;

    Summary summary(ctx, "");
    for (auto& h : held) {
        auto& st = h.state();
        for (auto it = st.instances.rbegin(); it != st.instances.rend(); ++it) summary.add(stopped_row(it->name, run::halt(*it)));
        st.instances.clear();
        st.roots.clear();
        h.store->save();
    }
    for (auto& l : loose) summary.add(stopped_row(l.name, process::stop_pid(l.pid, run::grace)));
    return done(summary);
}

/* Stops one node wherever on this machine it was started: by this workspace first, then by
 * another workspace, then without rant. */
int stop_node(app::Context& ctx, const std::string& name) {
    auto held = started_anywhere(ctx);
    const config::Workspace* ws = ctx.scope();
    auto own = [&](const Held& h) { return ws && util::path_key(h.root) == util::path_key(ws->root); };
    std::vector<std::pair<Held*, state::Instance*>> found;
    for (auto& h : held)
        if (auto* inst = h.state().instance(name)) found.push_back({ &h, inst });
    /* this workspace's own node wins over a namesake elsewhere */
    if (!found.empty() && own(*found[0].first)) found.resize(1);

    if (found.size() > 1) {
        std::vector<std::string> where;
        for (auto& f : found) where.push_back(ctx.shown(f.first->root));
        ctx.out.note("`" + name + "` was started in " + std::to_string(found.size()) + " workspaces: " + list(where));
        if (!ui::confirm("Stop all of them?", ctx.yes)) return 1;
    }
    for (auto& [h, inst] : found) {
        auto groups = groups_of(*inst);
        std::string in = own(*h) ? "" : " in " + ctx.shown(h->root);
        if (!groups.empty() && !ui::confirm("`" + name + "` belongs to the running group " + list(groups) + in + ". Stop it anyway?", ctx.yes))
            return 1;
    }
    Summary summary(ctx, "");
    for (auto& [h, inst] : found) {
        Summary::Row row = stopped_row(name, run::halt(*inst));
        if (!own(*h)) row.detail += (row.detail.empty() ? "" : ", ") + std::string("started in ") + ctx.shown(h->root);
        run::forget(h->state(), name);
        h->store->save();
        summary.add(row);
    }
    if (!found.empty()) return done(summary);

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
        if (p.name == name && !p.here)
            throw app::Failure("`" + name + "` is running on " + p.host() + ", rant only stops nodes on this machine");
    throw app::Failure("no node `" + name + "` on domain " + std::to_string(ctx.domain()) + ", see `rant ls nodes`");
}

/* The group roots to stop: the one root params name, else every root of the group, which
 * takes a yes when there are several. The group's own name is looked up, so a bare name
 * finds a group in a package. */
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
    if (out.size() > 1) {
        std::vector<std::string> keys;
        for (auto& r : out) keys.push_back(r.key().substr(6));
        ctx.out.note("group `" + name + "` is running " + std::to_string(out.size()) + " times: " + list(keys));
        if (!ui::confirm("Stop all of them?", ctx.yes)) return {};
    }
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
    if (names_group(ctx, w[0])) return stop_group(ctx, w[0], { w.begin() + 1, w.end() });
    if (w.size() != 1) throw app::UsageError("a node takes no key=value, only a group does");
    return stop_node(ctx, w[0]);
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.running_groups();
        for (auto& n : r.running_nodes()) out.push_back(n);
        return { out };
    }
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command stop() {
    app::Command c{ "stop", "[<group> [key=value...] | <node>]",
                    "stop any node on this machine, all this workspace started when nothing is named",
                    app::Section::Workspace,
                    { { "all", 'a', "", "stop every node on this machine, whoever started it" } }, run };
    c.complete = complete_words;
    return c;
}

}
