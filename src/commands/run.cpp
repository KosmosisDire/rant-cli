#include <algorithm>
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

/* One node of a node type, named after it. Running the same type again gives the copy a
 * free name, talker_1, then talker_2, since two nodes of one name collide on the mesh. */
int run_node(app::Context& ctx, const std::string& ref) {
    config::Plan plan = config::plan_node(ctx.cwd(), ref);
    require_plan(ctx, plan);
    place_on_domain(ctx, plan);
    note_names(ctx, {}, { plan.instances.front().name });
    if (ctx.args.has("dry-run")) {
        print_plan(ctx, plan);
        return 0;
    }
    const config::Instance& inst = plan.instances.front();
    bool built = build_for(ctx, { plan });

    std::set<std::string> taken;
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    for (auto& p : mesh.peers()) taken.insert(p.name);

    state::Instance rec;
    {
        run::Session s(*plan.workspace);
        for (auto& i : s.state().instances) taken.insert(i.name);
        std::string name = inst.name;
        for (int n = 1; taken.count(name); n++) name = inst.name + "_" + std::to_string(n);

        state::Root root{ "node", name, {} };
        rec = run::spawn(s.workspace(), inst, name);
        rec.roots.push_back(root.key());
        s.state().roots.push_back(root);
        s.state().instances.push_back(rec);
        s.save();
    }
    Summary summary(ctx, "");
    summary.add(started_row(rec));
    int code = bring_up(ctx, summary, { rec }, "", "started");
    return built ? code : 1;
}

static void group_help(app::Context& ctx, const std::string& group) {
    config::GroupInfo g = config::describe_group(ctx.cwd(), group);
    if (!g.diagnostics.empty()) {
        for (auto& d : g.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    ctx.out.line("Usage: rant run " + g.name + (g.params.empty() ? "" : " [key=value...]"));
    if (!g.description.empty()) ctx.out.line("\n" + g.description);
    print_params(ctx, g.params);
}

/* A group root: its nodes start in plan order. A node already running for another root
 * with the same settings is shared, and a node of that name on the mesh that rant did not
 * start can stand in after a question. Every check runs before the first node starts. */
int run_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params) {
    if (ctx.args.has("help")) {
        group_help(ctx, group);
        return 0;
    }
    config::Plan plan = config::plan_group(ctx.cwd(), group, params);
    require_plan(ctx, plan);
    place_on_domain(ctx, plan);
    std::vector<std::string> nodes;
    for (auto& i : plan.instances) nodes.push_back(i.name);
    note_names(ctx, { plan.group }, nodes);
    if (ctx.args.has("dry-run")) {
        print_plan(ctx, plan);
        return 0;
    }
    state::Root root{ "group", plan.group, plan.params };

    mesh::Client mesh(ctx.domain());
    mesh.settle();
    std::optional<run::Session> held(std::in_place, *plan.workspace);
    run::Session& s = *held;
    auto managed = run::managed_peers(mesh, s.state());

    std::vector<std::string> actions;
    for (auto& inst : plan.instances) {
        if (state::Instance* running = s.state().instance(inst.name)) {
            if (!run::same_spec(*running, inst))
                throw app::Failure("`" + inst.name + "` is already running with other settings for " + running->roots.front() +
                                   ", stop it first or give one of them another name");
            bool mine = std::count(running->roots.begin(), running->roots.end(), root.key()) > 0;
            actions.push_back(mine ? "running" : "shared");
            continue;
        }
        bool external = false;
        for (auto& p : mesh.peers()) external |= p.name == inst.name && !managed.count(p.id);
        if (external) {
            if (!ui::confirm("`" + inst.name + "` is already running on the mesh, but rant did not start it. Use it for " +
                                 plan.group + "?", ctx.yes))
                throw app::Failure("nothing was started");
            actions.push_back("external");
            continue;
        }
        actions.push_back("started");
    }

    /* only what starts is built: a running node's program may be in use */
    config::Plan starting = plan;
    starting.instances.clear();
    for (size_t i = 0; i < plan.instances.size(); i++)
        if (actions[i] == "started") starting.instances.push_back(plan.instances[i]);
    bool built = build_for(ctx, { starting });

    if (!s.state().root(root.key())) s.state().roots.push_back(root);
    std::string shown;
    for (auto& [k, v] : plan.params) shown += (shown.empty() ? "" : " ") + k + "=" + v;
    Summary summary(ctx, plan.group, shown);
    std::vector<state::Instance> started;
    try {
        for (size_t i = 0; i < plan.instances.size(); i++) {
            const config::Instance& inst = plan.instances[i];
            if (actions[i] == "started") {
                state::Instance rec = run::spawn(s.workspace(), inst, inst.name);
                rec.roots.push_back(root.key());
                s.state().instances.push_back(rec);
                started.push_back(rec);
                summary.add(started_row(rec));
            } else if (actions[i] == "shared" || actions[i] == "running") {
                state::Instance* running = s.state().instance(inst.name);
                std::string with = actions[i] == "shared" ? held_by(running->roots, "shared with") : "";
                if (actions[i] == "shared") running->roots.push_back(root.key());
                if (running->group.empty()) running->group = inst.group;
                summary.add({ inst.name, Summary::Mark::Running, "already running", with });
            } else {
                summary.add({ inst.name, Summary::Mark::Running, "already running", "started outside rant" });
            }
        }
    } catch (...) {
        s.save();    /* what did start stays tracked */
        summary.finish();
        throw;
    }
    s.save();
    held.reset();    /* unlocks the state before the watch */
    int code = bring_up(ctx, summary, started, plan.group, "started");
    return built ? code : 1;
}

static void fail_with(app::Context& ctx, const std::vector<config::Diagnostic>& diagnostics) {
    for (auto& d : diagnostics) ctx.out.error(d.str());
    throw app::Failure("");
}

std::string group_name(app::Context& ctx, const std::string& group) {
    config::GroupInfo info = config::describe_group(ctx.cwd(), group);
    return info.diagnostics.empty() ? info.name : group;
}

bool names_group(app::Context& ctx, const std::string& name) {
    if (!ctx.workspace()) return false;
    config::Named n = config::named(ctx.cwd(), name);
    if (n.group && n.node) fail_with(ctx, n.diagnostics);
    if (n.group) return true;
    for (auto& r : run::snapshot(ctx).roots)
        if (r.kind == "group" && r.name == name) return true;
    return false;
}

/* A group is what a system runs as, so a bare name is a group. One node alone is asked for
 * with the word node, for trying it out. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    bool node = !w.empty() && w[0] == "node";
    std::optional<config::Named> n;
    if (!w.empty() && !node) n = config::named(ctx.cwd(), w[0]);
    if (ctx.args.has("help") && !(n && n->group)) {
        ctx.out.line(app::command_help(run_()));
        return 0;
    }
    if (w.empty()) throw app::UsageError("say which group to run: `rant run <group>`, or one node with `rant run node <node>`");
    if (node) {
        if (w.size() != 2) throw app::UsageError("run node takes one node, such as `rant run node talker`");
        return run_node(ctx, w[1]);
    }
    if (n->group) return run_group(ctx, w[0], { w.begin() + 1, w.end() });
    if (n->node) throw app::UsageError("`" + w[0] + "` is a node, run it in a group or alone with `rant run node " + w[0] + "`");
    throw app::Failure("no group named `" + w[0] + "`, see `rant ls groups`");
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.groups();
        out.push_back("node");
        return { out };
    }
    if (w[0] == "node") return { w.size() == 1 ? r.node_types() : std::vector<std::string>{} };
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command run_() {
    app::Command c{ "run", "<group> [key=value...] | node <node>",
                    "start a group, or one node, detached, logging to the logs folder", app::Section::Workspace,
                    { { "dry-run", 0, "", "show what would run without starting it" }, no_build_option(), wait_option(),
                      watch_option() },
                    run };
    c.own_help = true;
    c.complete = complete_words;
    return c;
}

}
