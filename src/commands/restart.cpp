#include <algorithm>
#include <set>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "commands/start.hpp"
#include "complete/complete.hpp"
#include "run/nodes.hpp"
#include "ui/prompt.hpp"

namespace commands {

static std::vector<std::string> param_words(const state::Root& r) {
    std::vector<std::string> out;
    for (auto& [k, v] : r.params) out.push_back(k + "=" + v);
    return out;
}

/* The plan a root runs now, from the config as it stands, so a restart takes in edits and
 * new builds. Planned before anything stops, so a broken config stops nothing. */
static config::Plan replan(app::Context& ctx, const state::Root& root, const state::Instance* node) {
    config::Plan plan = root.kind == "group" ? config::plan_group(ctx.cwd, root.name, param_words(root))
                                             : config::plan_node(ctx.cwd, node ? node->type : root.name);
    require_plan(ctx, plan);
    return plan;
}

static void report_stop(app::Context& ctx, const std::string& name, process::Stopped how) {
    if (how == process::Stopped::Killed) ctx.out.note(name + " did not stop within " + std::to_string(run::grace.count()) + " s and was killed");
}

/* Restarts one node under its name and roots, with the spec its first root plans now. */
static void restart_instance(app::Context& ctx, run::Session& s, const std::string& name, const config::Instance& planned) {
    state::Instance* inst = s.state().instance(name);
    report_stop(ctx, name, run::halt(*inst));
    std::vector<std::string> roots = inst->roots;
    state::Instance rec = run::spawn(s.workspace(), planned, name);
    rec.roots = roots;
    *s.state().instance(name) = rec;
    s.save();
    ctx.out.line("restarted " + name + ", pid " + std::to_string(rec.tracking.pid));
}

static int restart_node(app::Context& ctx, const std::string& name) {
    run::Session s(ctx.require_workspace());
    state::Instance* inst = s.state().instance(name);
    if (!inst) throw app::Failure("rant runs no node named `" + name + "` here, see `rant ls`");
    const state::Root* root = s.state().root(inst->roots.front());
    if (!root) throw app::Failure("`" + name + "` has lost its root, stop it and start it again");
    config::Plan plan = replan(ctx, *root, inst);
    const config::Instance* planned = nullptr;
    for (auto& i : plan.instances)
        if (root->kind == "node" || i.name == name) planned = &i;
    if (!planned) throw app::Failure("`" + name + "` is no longer part of " + root->key().substr(6) + ", stop it instead");
    restart_instance(ctx, s, name, *planned);
    return 0;
}

/* Restarts every node of one group root: its nodes stop last started first, then the plan
 * starts in order. A node the plan has dropped stops for good unless another root needs it. */
static void restart_root(app::Context& ctx, run::Session& s, const state::Root& root) {
    config::Plan plan = replan(ctx, root, nullptr);
    state::State& st = s.state();
    std::string key = root.key();
    std::vector<std::string> mine;
    for (auto& i : st.instances)
        if (std::count(i.roots.begin(), i.roots.end(), key)) mine.push_back(i.name);
    for (auto it = mine.rbegin(); it != mine.rend(); ++it) {
        state::Instance* inst = st.instance(*it);
        bool planned = std::any_of(plan.instances.begin(), plan.instances.end(), [&](const config::Instance& p) { return p.name == *it; });
        if (!planned && inst->roots.size() > 1) {
            inst->roots.erase(std::remove(inst->roots.begin(), inst->roots.end(), key), inst->roots.end());
            continue;    /* another root still needs it as it is */
        }
        report_stop(ctx, *it, run::halt(*inst));
        if (!planned) {
            st.instances.erase(std::remove_if(st.instances.begin(), st.instances.end(), [&](const state::Instance& i) { return i.name == *it; }),
                               st.instances.end());
            ctx.out.line("stopped " + *it + ", no longer in " + key.substr(6));
        }
    }
    s.save();

    /* a node the plan has that rant does not run may run on the mesh without rant */
    std::set<std::string> elsewhere;
    if (std::any_of(plan.instances.begin(), plan.instances.end(), [&](const config::Instance& p) { return !st.instance(p.name); })) {
        mesh::Client mesh(ctx.domain());
        mesh.settle();
        auto managed = run::managed_peers(mesh, st);
        for (auto& peer : mesh.peers())
            if (!managed.count(peer.id)) elsewhere.insert(peer.name);
    }
    for (auto& p : plan.instances) {
        state::Instance* inst = st.instance(p.name);
        if (inst && !std::count(inst->roots.begin(), inst->roots.end(), key)) {
            inst->roots.push_back(key);    /* another root's node, now shared, left running as it is */
            continue;
        }
        if (!inst && elsewhere.count(p.name)) {
            ctx.out.line("left " + p.name + " running, rant did not start it");
            continue;
        }
        std::vector<std::string> roots = inst ? inst->roots : std::vector<std::string>{ key };
        state::Instance rec = run::spawn(s.workspace(), p, p.name);
        rec.roots = roots;
        if (inst) *inst = rec;
        else st.instances.push_back(rec);
        s.save();
        ctx.out.line("restarted " + p.name + ", pid " + std::to_string(rec.tracking.pid));
    }
}

/* The running roots of a group: the one its params name, else every one of it. Empty when
 * it is not running. */
static std::vector<state::Root> group_roots(app::Context& ctx, const state::State& st, const std::string& group,
                                            const std::vector<std::string>& params) {
    std::vector<state::Root> out;
    if (!params.empty()) {
        config::Plan plan = config::plan_group(ctx.cwd, group, params);
        require_plan(ctx, plan);
        state::Root root{ "group", plan.group, plan.params };
        for (auto& r : st.roots)
            if (r.key() == root.key()) out.push_back(r);
        return out;
    }
    config::GroupInfo info = config::describe_group(ctx.cwd, group);
    std::string name = info.diagnostics.empty() ? info.name : group;
    for (auto& r : st.roots)
        if (r.kind == "group" && r.name == name) out.push_back(r);
    return out;
}

static int restart_all(app::Context& ctx) {
    run::Session s(ctx.require_workspace());
    std::vector<state::Root> roots = s.state().roots;
    if (roots.empty()) {
        ctx.out.line("nothing rant started is running");
        return 0;
    }
    std::string list;
    for (auto& r : roots) list += (list.empty() ? "" : ", ") + r.key();
    ctx.out.line("roots: " + list);
    if (!ui::confirm("Restart all " + std::to_string(s.state().instances.size()) + " nodes?", ctx.yes)) return 1;
    for (auto& r : roots) {
        if (r.kind == "group") {
            restart_root(ctx, s, r);
            continue;
        }
        for (auto& i : s.state().instances)
            if (i.roots.front() == r.key()) {
                config::Plan plan = replan(ctx, r, &i);
                restart_instance(ctx, s, i.name, plan.instances.front());
                break;
            }
    }
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) return restart_all(ctx);
    if (w.size() < 2 || (w[0] != "node" && w[0] != "group"))
        throw app::UsageError("say what to restart: `rant restart node <name>` or `rant restart group <name>`");
    /* what is not running starts, looked up first without holding the state, which start takes */
    if (w[0] == "node") {
        if (w.size() != 2) throw app::UsageError("restart node takes one name");
        if (run::snapshot(ctx).instance(w[1])) return restart_node(ctx, w[1]);
        ctx.out.note("`" + w[1] + "` is not running, starting it");
        return start_node(ctx, w[1]);
    }
    std::vector<std::string> params(w.begin() + 2, w.end());
    if (group_roots(ctx, run::snapshot(ctx), w[1], params).empty()) {
        ctx.out.note("group `" + w[1] + "` is not running, starting it");
        return start_group(ctx, w[1], params);
    }
    run::Session s(ctx.require_workspace());
    for (auto& r : group_roots(ctx, s.state(), w[1], params)) restart_root(ctx, s, r);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) return { { "node", "group" } };
    if (w[0] == "node" && w.size() == 1) {
        std::vector<std::string> out = r.running_nodes();
        for (auto& n : r.node_types()) out.push_back(n);
        return { out };
    }
    if (w[0] != "group") return {};
    if (w.size() == 1) return { r.groups() };
    return { complete::params(r.group(w[1]), r.partial, { w.begin() + 2, w.end() }) };
}

app::Command restart() {
    app::Command c{ "restart", "[node <name> | group <name> [key=value...]]",
                    "stop and start again as configured now, start what is not running, all of it when nothing is named",
                    app::Section::Workspace, {}, run };
    c.complete = complete_words;
    return c;
}

}
