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

/* A restarted node's row, saying so when the old one had to be killed. */
static Summary::Row restarted_row(const state::Instance& rec, process::Stopped how) {
    Summary::Row r = started_row(rec);
    if (how == process::Stopped::Killed || how == process::Stopped::KilledAtOnce) r.detail = "the old one killed";
    return r;
}

/* A root to restart: its plan as configured now, and the one node of it when only that. */
struct Target {
    state::Root  root;
    config::Plan plan;
    std::string  only;
};

/* What a restart stopped of one root, to start again once the build is done. */
struct Halted {
    Target                                  target;
    std::map<std::string, process::Stopped> how;
    std::vector<Summary::Row>               dropped;    /* nodes the plan no longer has */
};

/* Stops a root's nodes, last started first. A node the plan has dropped stops for good
 * unless another group needs it. */
static Halted halt_root(run::Session& s, const Target& t) {
    Halted h{ t, {}, {} };
    state::State& st = s.state();
    std::string key = t.root.key();
    std::vector<std::string> mine;
    for (auto& i : st.instances)
        if (std::count(i.roots.begin(), i.roots.end(), key) && (t.only.empty() || i.name == t.only)) mine.push_back(i.name);
    for (auto it = mine.rbegin(); it != mine.rend(); ++it) {
        state::Instance* inst = st.instance(*it);
        bool planned = std::any_of(t.plan.instances.begin(), t.plan.instances.end(), [&](const config::Instance& p) { return p.name == *it; });
        if (!planned) {
            inst->roots.erase(std::remove(inst->roots.begin(), inst->roots.end(), key), inst->roots.end());
            if (run::held_by_group(*inst)) continue;    /* another group still needs it as it is */
        }
        h.how[*it] = run::halt(*inst);
        if (!planned) {
            run::forget(st, *it);
            h.dropped.push_back({ *it, Summary::Mark::Stopped, "stopped", "no longer in " + t.root.name });
        }
    }
    s.save();
    return h;
}

/* Starts a halted root's plan in order, each node under its old name and roots. */
static void start_root(app::Context& ctx, run::Session& s, const Halted& h, std::vector<state::Instance>& started, Summary& summary) {
    for (auto& r : h.dropped) summary.add(r);
    state::State& st = s.state();
    std::string key = h.target.root.key();
    const auto& planned = h.target.plan.instances;
    /* a node the plan has that rant does not run may run on the mesh without rant */
    std::set<std::string> elsewhere;
    if (std::any_of(planned.begin(), planned.end(), [&](const config::Instance& p) { return !st.instance(p.name); })) {
        mesh::Client mesh(ctx.domain());
        mesh.settle();
        auto managed = run::managed_peers(mesh, st);
        for (auto& peer : mesh.peers())
            if (!managed.count(peer.id)) elsewhere.insert(peer.name);
    }
    for (auto& p : planned) {
        state::Instance* inst = st.instance(p.name);
        if (inst && !std::count(inst->roots.begin(), inst->roots.end(), key)) {
            std::string with = held_by(inst->roots, "shared with");
            inst->roots.push_back(key);    /* another root's node, now shared, left running as it is */
            summary.add({ p.name, Summary::Mark::Running, "already running", with });
            continue;
        }
        if (!inst && elsewhere.count(p.name)) {
            summary.add({ p.name, Summary::Mark::Running, "already running", "started outside rant" });
            continue;
        }
        std::vector<std::string> roots = inst ? inst->roots : std::vector<std::string>{ key };
        /* a node comes back in the session it ran in */
        state::Instance rec = run::spawn(s.workspace(), p, p.name, inst ? std::optional<uint32_t>(inst->session) : std::nullopt);
        rec.roots = roots;
        if (inst) *inst = rec;
        else st.instances.push_back(rec);
        s.save();
        started.push_back(rec);
        auto stopped = h.how.find(p.name);
        summary.add(restarted_row(rec, stopped == h.how.end() ? process::Stopped::AlreadyGone : stopped->second));
    }
}

/* Stops the targets, builds them, then starts them again, with the state locked throughout
 * so nothing else starts these nodes meanwhile. Returns whether the build worked. */
static bool restart(app::Context& ctx, run::Session& s, const std::vector<Target>& targets, std::vector<state::Instance>& started,
                    Summary& summary) {
    std::vector<Halted> halted;
    std::vector<config::Plan> plans;
    for (auto& t : targets) {
        plans.push_back(t.plan);
        halted.push_back(halt_root(s, t));
    }
    bool built = build_for(ctx, plans);
    for (auto& h : halted) start_root(ctx, s, h, started, summary);
    return built;
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
    std::string name = group_name(ctx, group);
    for (auto& r : st.roots)
        if (r.kind == "group" && r.name == name) out.push_back(r);
    return out;
}

/* Every root this workspace started, each planned before anything stops, so a broken
 * config stops nothing. */
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

    std::vector<state::Instance> started;
    Summary summary(ctx, "");
    bool built;
    {
        run::Session s(ctx.require_scope());
        std::vector<Target> targets;
        for (auto& r : s.state().roots) {
            if (r.kind == "group") {
                targets.push_back({ r, replan(ctx, r, nullptr), "" });
                continue;
            }
            for (auto& i : s.state().instances)
                if (i.roots.front() == r.key()) {
                    targets.push_back({ r, replan(ctx, r, &i), i.name });
                    break;
                }
        }
        built = restart(ctx, s, targets, started, summary);
    }    /* the state unlocks before the wait, so other commands need not wait */
    int code = bring_up(ctx, summary, ctx.require_scope(), started, "", "restarted");
    return built ? code : 1;
}

/* A running node alone, as the root that started it plans it now. */
static Target node_target(app::Context& ctx, state::State& st, const state::Instance& inst) {
    const state::Root* root = st.root(inst.roots.front());
    if (!root) throw app::Failure("rant lost track of `" + inst.name + "`, stop and start it again");
    config::Plan plan = replan(ctx, *root, &inst);
    if (root->kind == "group")
        plan.instances.erase(std::remove_if(plan.instances.begin(), plan.instances.end(),
                                            [&](const config::Instance& i) { return i.name != inst.name; }),
                             plan.instances.end());
    if (plan.instances.empty()) throw app::Failure("`" + inst.name + "` is no longer in " + root->key().substr(6) + ", stop it instead");
    return { *root, plan, inst.name };
}

/* Running nodes by name, each under its name and roots. header heads the summary. */
static int restart_nodes(app::Context& ctx, const std::vector<std::string>& names, const std::string& header) {
    std::vector<state::Instance> started;
    Summary summary(ctx, header);
    bool built;
    {
        run::Session s(ctx.require_scope());
        std::vector<Target> targets;
        for (auto& name : names) {
            state::Instance* inst = s.state().instance(name);
            if (!inst) throw app::Failure("no node `" + name + "` started here, see `rant ls nodes`");
            targets.push_back(node_target(ctx, s.state(), *inst));
        }
        built = restart(ctx, s, targets, started, summary);
    }    /* the state unlocks before the wait, so other commands need not wait */
    int code = bring_up(ctx, summary, ctx.require_scope(), started, header, "restarted");
    return built ? code : 1;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) return restart_all(ctx);
    /* what is not running starts, looked up first without holding the state, which run takes */
    if (!names_group(ctx, w[0])) {
        if (w.size() != 1) throw app::UsageError("a node takes no key=value, only a group does");
        if (run::snapshot(ctx).instance(w[0])) return restart_nodes(ctx, { w[0] }, "");
        ctx.out.note("`" + w[0] + "` is not running, starting it");
        return run_node(ctx, w[0]);
    }
    std::vector<std::string> params(w.begin() + 1, w.end());
    if (group_roots(ctx, run::snapshot(ctx), w[0], params).empty()) {
        /* a group that runs only inside others restarts its own nodes there */
        std::string name = group_name(ctx, w[0]);
        auto inside = run::made_by(run::snapshot(ctx), name);
        if (params.empty() && !inside.empty()) return restart_nodes(ctx, inside, name);
        ctx.out.note("group `" + w[0] + "` is not running, starting it");
        return run_group(ctx, w[0], params);
    }
    std::vector<state::Instance> started;
    std::optional<Summary> summary;
    std::string group;
    bool built;
    {
        run::Session s(ctx.require_scope());
        auto roots = group_roots(ctx, s.state(), w[0], params);
        group = roots.front().name;
        std::vector<Target> targets;
        for (auto& r : roots) targets.push_back({ r, replan(ctx, r, nullptr), "" });
        summary.emplace(ctx, group);
        built = restart(ctx, s, targets, started, *summary);
    }    /* the state unlocks before the wait, so other commands need not wait */
    int code = bring_up(ctx, *summary, ctx.require_scope(), started, group, "restarted");
    return built ? code : 1;
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
                    app::Section::Workspace, { no_build_option(), wait_option(), watch_option() }, run };
    c.complete = complete_words;
    return c;
}

}
