#include <algorithm>
#include <map>
#include <optional>
#include <set>

#include "app/failure.hpp"
#include "commands/build.hpp"
#include "commands/commands.hpp"
#include "commands/enable.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "commands/summary.hpp"
#include "complete/complete.hpp"
#include "run/nodes.hpp"
#include "ui/prompt.hpp"
#include "util/text.hpp"

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

/* A root to restart: its plan as configured now, the one node of it when only that, and the
 * nodes a service holds, which a restart of everything leaves be. */
struct Target {
    state::Root           root;
    config::Plan          plan;
    std::string           only;
    std::set<std::string> keep;
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
        if (std::count(i.roots.begin(), i.roots.end(), key) && (t.only.empty() || i.name == t.only) && !t.keep.count(i.name))
            mine.push_back(i.name);
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
        if (inst && h.target.keep.count(p.name)) {
            summary.add({ p.name, Summary::Mark::Running, "kept running", "a service holds it" });
            continue;
        }
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

/* Every root this workspace started but its services, each planned before anything stops, so a
 * broken config stops nothing. */
static int restart_all(app::Context& ctx) {
    state::State now = run::snapshot(ctx);
    Kept k = kept_services(now, ctx.require_scope().data);
    std::vector<std::string> keys;
    for (auto& r : now.roots)
        if (!k.roots.count(r.key())) keys.push_back(r.key());
    if (keys.empty()) {
        if (k.running.empty()) ctx.out.line("nothing rant started is running");
        else ctx.out.line("only services run here, name one to restart it: " + util::joined(k.running));
        return 0;
    }
    ctx.out.line("roots: " + util::joined(keys));
    if (!k.running.empty()) ctx.out.note("services keep running, name one to restart it: " + util::joined(k.running));
    auto count = std::count_if(now.instances.begin(), now.instances.end(), [&](const state::Instance& i) { return !k.nodes.count(i.name); });
    if (!ui::confirm("Restart all " + std::to_string(count) + " nodes?", ctx.yes)) return 1;

    std::vector<state::Instance> started;
    Summary summary(ctx, "");
    bool built;
    {
        run::Session s(ctx.require_scope());
        Kept held = kept_services(s.state(), s.workspace().data);
        std::vector<Target> targets;
        for (auto& r : s.state().roots) {
            if (held.roots.count(r.key())) continue;
            if (r.kind == "group") {
                targets.push_back({ r, replan(ctx, r, nullptr), "", held.nodes });
                continue;
            }
            for (auto& i : s.state().instances)
                if (i.roots.front() == r.key()) {
                    if (!held.nodes.count(i.name)) targets.push_back({ r, replan(ctx, r, &i), i.name, {} });
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

/* One node by name, started when it is not running. */
static int restart_node(app::Context& ctx, const std::string& name) {
    state::State st = run::snapshot(ctx);
    if (const state::Instance* i = st.instance(name)) return restart_nodes(ctx, { i->name }, "");
    ctx.out.note("`" + name + "` is not running, starting it");
    return run_node(ctx, name);
}

/* A bare name restarted in this workspace: a group of it, else the node, started when it is
 * not running. */
static int restart_named(app::Context& ctx, const std::string& name, const std::vector<std::string>& params) {
    /* what is not running starts, looked up first without holding the state, which run takes */
    if (!names_group(ctx, name)) {
        if (!params.empty()) throw app::UsageError("a node takes no key=value, only a group does");
        return restart_node(ctx, name);
    }
    if (group_roots(ctx, run::snapshot(ctx), name, params).empty()) {
        /* a group that runs only inside others restarts its own nodes there */
        std::string full = group_name(ctx, name);
        auto inside = run::made_by(run::snapshot(ctx), full);
        if (params.empty() && !inside.empty()) return restart_nodes(ctx, inside, full);
        ctx.out.note("group `" + name + "` is not running, starting it");
        return run_group(ctx, name, params);
    }
    std::vector<state::Instance> started;
    std::optional<Summary> summary;
    std::string group;
    bool built;
    {
        run::Session s(ctx.require_scope());
        auto roots = group_roots(ctx, s.state(), name, params);
        group = roots.front().name;
        std::vector<Target> targets;
        for (auto& r : roots) targets.push_back({ r, replan(ctx, r, nullptr), "" });
        summary.emplace(ctx, group);
        built = restart(ctx, s, targets, started, *summary);
    }    /* the state unlocks before the wait, so other commands need not wait */
    int code = bring_up(ctx, *summary, ctx.require_scope(), started, group, "restarted");
    return built ? code : 1;
}

/* The node a node type's reference means: the one running under the name that type starts
 * with on this machine. Another under that name or of that type is not it. */
static std::optional<std::string> running_as(app::Context& ctx, const std::string& ref) {
    config::Plan plan = config::plan_node(ctx.cwd(), ref);
    if (plan.instances.empty()) return std::nullopt;
    const config::Instance& p = plan.instances.front();
    for (auto& [root, st] : states_anywhere(ctx, false))
        for (auto& i : st.instances)
            if (i.name == p.name && i.type == p.type) return i.name;
    return std::nullopt;
}

/* Restarts a name where it runs on this machine, as stop finds it, else starts it here. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) return restart_all(ctx);
    bool node = w[0] == "node";
    if (node && w.size() != 2) throw app::UsageError("restart node takes one node, such as `rant restart node talker`");
    std::string name = node ? w[1] : w[0];
    auto where = where_running(ctx, name, node, "restart");
    if (!where) return 1;
    if (where->empty() && (node || (w.size() == 1 && !names_group(ctx, name))))
        if (auto running = running_as(ctx, name)) {
            name = *running;
            node = true;
            where = where_running(ctx, name, node, "restart");
            if (!where) return 1;
        }
    std::vector<std::string> params = node ? std::vector<std::string>{} : resolved_params(ctx, name, { w.begin() + 1, w.end() });
    auto act = [&](const std::string& in) {
        if (!in.empty()) ctx.out.note("`" + name + "` runs in " + in);
        return node ? restart_node(ctx, name) : restart_named(ctx, name, params);
    };
    if (where->empty()) return act("");
    return in_each(ctx, *where, act);
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.groups();
        for (auto& n : r.running_nodes()) out.push_back(n);
        for (auto& n : r.node_types()) out.push_back(n);
        out.push_back("node");
        return { out };
    }
    if (w[0] == "node") {
        if (w.size() != 1) return {};
        std::vector<std::string> out = r.running_nodes();
        for (auto& n : r.node_types()) out.push_back(n);
        return { out };
    }
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command restart() {
    app::Command c{ "restart", "[<group> [key=value...] | [node] <node>]",
                    "stop and start again as configured now, start what is not running, all of it when nothing is named",
                    app::Section::Workspace, { no_build_option(), wait_option(), watch_option() }, run };
    c.complete = complete_words;
    return c;
}

}
