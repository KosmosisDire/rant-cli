#include <algorithm>
#include <map>
#include <optional>
#include <set>

#include "app/failure.hpp"
#include "commands/build.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "commands/summary.hpp"
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
    config::Plan plan = root.kind == "group" ? config::plan_group(ctx.cwd(), root.name, param_words(root))
                                             : config::plan_node(ctx.cwd(), node ? node->type : root.name);
    require_plan(ctx, plan);
    place_on_domain(ctx, plan);
    return plan;
}

/* Builds the packages whose nodes the plans run, and what they depend on, so a restart runs
 * the code as it is now. A failed build throws before anything stops. */
static void rebuild(app::Context& ctx, const std::vector<config::Plan>& plans) {
    const config::Workspace& ws = ctx.require_workspace(true);
    std::vector<std::string> packages;
    for (auto& plan : plans)
        for (auto& i : plan.instances) {
            std::string pkg = i.type.substr(0, i.type.find('/'));
            bool known = std::any_of(ws.packages.begin(), ws.packages.end(), [&](const config::Package& p) { return p.name == pkg; });
            if (known && std::find(packages.begin(), packages.end(), pkg) == packages.end()) packages.push_back(pkg);
        }
    if (packages.empty()) return;
    config::Build b = config::plan_build(ctx.cwd(), packages);
    if (!b.diagnostics.empty()) {
        for (auto& d : b.diagnostics) ctx.out.error(d.str());
        throw app::Failure("nothing was restarted");
    }
    try {
        if (build_steps(ctx, b) > 0) ctx.out.line();
    } catch (const app::Failure& e) {
        throw app::Failure(std::string(e.what()) + ", nothing was restarted");
    }
}

/* A restarted node's row, saying so when the old one had to be killed. */
static Summary::Row restarted_row(const state::Instance& rec, process::Stopped how) {
    Summary::Row r = started_row(rec);
    if (how == process::Stopped::Killed || how == process::Stopped::KilledAtOnce) r.detail = "the old one killed";
    return r;
}

/* Restarts one node under its name and roots, with the spec its first root plans now. */
static state::Instance restart_instance(run::Session& s, const std::string& name, const config::Instance& planned,
                                        Summary& summary) {
    state::Instance* inst = s.state().instance(name);
    process::Stopped how = run::halt(*inst);
    std::vector<std::string> roots = inst->roots;
    state::Instance rec = run::spawn(s.workspace(), planned, name);
    rec.roots = roots;
    *s.state().instance(name) = rec;
    s.save();
    summary.add(restarted_row(rec, how));
    return rec;
}

/* The restarted node, returned once the state is unlocked again. */
static state::Instance restart_node(app::Context& ctx, const std::string& name, Summary& summary) {
    run::Session s(ctx.require_workspace());
    state::Instance* inst = s.state().instance(name);
    if (!inst) throw app::Failure("no node `" + name + "` started here, see `rant ls nodes`");
    const state::Root* root = s.state().root(inst->roots.front());
    if (!root) throw app::Failure("rant lost track of `" + name + "`, stop and start it again");
    config::Plan plan = replan(ctx, *root, inst);
    const config::Instance* planned = nullptr;
    for (auto& i : plan.instances)
        if (root->kind == "node" || i.name == name) planned = &i;
    if (!planned) throw app::Failure("`" + name + "` is no longer in " + root->key().substr(6) + ", stop it instead");
    return restart_instance(s, name, *planned, summary);
}

/* Restarts every node of one group root: its nodes stop last started first, then the plan
 * starts in order. A node the plan has dropped stops for good unless another root needs it. */
static void restart_root(app::Context& ctx, run::Session& s, const state::Root& root, std::vector<state::Instance>& started,
                         Summary& summary) {
    config::Plan plan = replan(ctx, root, nullptr);
    state::State& st = s.state();
    std::string key = root.key();
    std::vector<std::string> mine;
    for (auto& i : st.instances)
        if (std::count(i.roots.begin(), i.roots.end(), key)) mine.push_back(i.name);
    std::map<std::string, process::Stopped> how;
    for (auto it = mine.rbegin(); it != mine.rend(); ++it) {
        state::Instance* inst = st.instance(*it);
        bool planned = std::any_of(plan.instances.begin(), plan.instances.end(), [&](const config::Instance& p) { return p.name == *it; });
        if (!planned && inst->roots.size() > 1) {
            inst->roots.erase(std::remove(inst->roots.begin(), inst->roots.end(), key), inst->roots.end());
            continue;    /* another root still needs it as it is */
        }
        how[*it] = run::halt(*inst);
        if (!planned) {
            st.instances.erase(std::remove_if(st.instances.begin(), st.instances.end(), [&](const state::Instance& i) { return i.name == *it; }),
                               st.instances.end());
            summary.add({ *it, Summary::Mark::Stopped, "stopped", "no longer in " + root.name });
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
            std::string with;
            for (auto& k : inst->roots)
                if (const state::Root* r = st.root(k)) with += (with.empty() ? "" : ", ") + r->name;
            inst->roots.push_back(key);    /* another root's node, now shared, left running as it is */
            summary.add({ p.name, Summary::Mark::Running, "already running", "shared with " + with });
            continue;
        }
        if (!inst && elsewhere.count(p.name)) {
            summary.add({ p.name, Summary::Mark::Running, "already running", "started outside rant" });
            continue;
        }
        std::vector<std::string> roots = inst ? inst->roots : std::vector<std::string>{ key };
        state::Instance rec = run::spawn(s.workspace(), p, p.name);
        rec.roots = roots;
        if (inst) *inst = rec;
        else st.instances.push_back(rec);
        s.save();
        started.push_back(rec);
        auto stopped = how.find(p.name);
        summary.add(restarted_row(rec, stopped == how.end() ? process::Stopped::AlreadyGone : stopped->second));
    }
}

/* The running roots of a group: the one its params name, else every one of it. Empty when
 * it is not running. */
static std::vector<state::Root> group_roots(app::Context& ctx, const state::State& st, const std::string& group,
                                            const std::vector<std::string>& params) {
    std::vector<state::Root> out;
    if (!params.empty()) {
        config::Plan plan = config::plan_group(ctx.cwd(), group, params);
        require_plan(ctx, plan);
        state::Root root{ "group", plan.group, plan.params };
        for (auto& r : st.roots)
            if (r.key() == root.key()) out.push_back(r);
        return out;
    }
    config::GroupInfo info = config::describe_group(ctx.cwd(), group);
    std::string name = info.diagnostics.empty() ? info.name : group;
    for (auto& r : st.roots)
        if (r.kind == "group" && r.name == name) out.push_back(r);
    return out;
}

static int restart_all(app::Context& ctx) {
    state::State now = run::snapshot(ctx);
    if (now.roots.empty()) {
        ctx.out.line("nothing rant started is running");
        return 0;
    }
    std::string list;
    for (auto& r : now.roots) list += (list.empty() ? "" : ", ") + r.key();
    ctx.out.line("roots: " + list);
    if (!ui::confirm("Restart all " + std::to_string(now.instances.size()) + " nodes?", ctx.yes)) return 1;
    std::vector<config::Plan> plans;
    for (auto& r : now.roots) {
        const state::Instance* first = nullptr;
        for (auto& i : now.instances)
            if (!first && i.roots.front() == r.key()) first = &i;
        if (r.kind == "group" || first) plans.push_back(replan(ctx, r, first));
    }
    rebuild(ctx, plans);

    std::vector<state::Instance> started;
    Summary summary(ctx, "");
    {
        run::Session s(ctx.require_workspace());
        std::vector<state::Root> roots = s.state().roots;
        for (auto& r : roots) {
            if (r.kind == "group") {
                restart_root(ctx, s, r, started, summary);
                continue;
            }
            for (auto& i : s.state().instances)
                if (i.roots.front() == r.key()) {
                    config::Plan plan = replan(ctx, r, &i);
                    started.push_back(restart_instance(s, i.name, plan.instances.front(), summary));
                    break;
                }
        }
    }
    return bring_up(ctx, summary, started, "", "restarted");
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) return restart_all(ctx);
    /* what is not running starts, looked up first without holding the state, which run takes */
    if (!names_group(ctx, w[0])) {
        if (w.size() != 1) throw app::UsageError("a node takes no key=value, only a group does");
        state::State now = run::snapshot(ctx);
        if (const state::Instance* inst = now.instance(w[0])) {
            if (const state::Root* root = now.root(inst->roots.front())) rebuild(ctx, { replan(ctx, *root, inst) });
            Summary summary(ctx, "");
            state::Instance rec = restart_node(ctx, w[0], summary);
            return bring_up(ctx, summary, { rec }, "", "restarted");
        }
        ctx.out.note("`" + w[0] + "` is not running, starting it");
        return run_node(ctx, w[0]);
    }
    std::vector<std::string> params(w.begin() + 1, w.end());
    if (group_roots(ctx, run::snapshot(ctx), w[0], params).empty()) {
        ctx.out.note("group `" + w[0] + "` is not running, starting it");
        return run_group(ctx, w[0], params);
    }
    {
        std::vector<config::Plan> plans;
        for (auto& r : group_roots(ctx, run::snapshot(ctx), w[0], params)) plans.push_back(replan(ctx, r, nullptr));
        rebuild(ctx, plans);
    }
    std::vector<state::Instance> started;
    std::optional<Summary> summary;
    std::string group;
    {
        run::Session s(ctx.require_workspace());
        auto roots = group_roots(ctx, s.state(), w[0], params);
        group = roots.front().name;
        summary.emplace(ctx, group);
        for (auto& r : roots) restart_root(ctx, s, r, started, *summary);
    }    /* the state unlocks before the watch, so other commands need not wait */
    return bring_up(ctx, *summary, started, group, "restarted");
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.groups();
        for (auto& n : r.running_nodes()) out.push_back(n);
        for (auto& n : r.node_types()) out.push_back(n);
        return { out };
    }
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command restart() {
    app::Command c{ "restart", "[<group> [key=value...] | <node>]",
                    "stop and start again as configured now, start what is not running, all of it when nothing is named",
                    app::Section::Workspace, { wait_option(), watch_option() }, run };
    c.complete = complete_words;
    return c;
}

}
