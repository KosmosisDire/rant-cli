#include <algorithm>
#include <set>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "complete/complete.hpp"
#include "run/nodes.hpp"
#include "ui/prompt.hpp"

namespace commands {

using json = nlohmann::ordered_json;

static std::string relative(const config::Workspace& ws, const std::string& path) {
    std::error_code ec;
    auto rel = config::fs::relative(config::from_utf8(path), ws.root, ec);
    return ec || rel.empty() ? path : config::to_utf8(rel);
}

/* What start did for one node, printed as a line or a JSON object. */
struct Outcome {
    std::string name;
    std::string action;    /* "started", "running", "shared" or "external" */
    uint64_t    pid = 0;
    std::string log;
};

static void report(app::Context& ctx, const config::Workspace& ws, const std::vector<Outcome>& done) {
    if (ctx.json) {
        json out = json::array();
        for (auto& o : done) {
            json j = { { "name", o.name }, { "action", o.action } };
            if (o.action == "started") j["pid"] = o.pid, j["log"] = o.log;
            out.push_back(j);
        }
        ctx.out.line(json{ { "nodes", out } }.dump(2));
        return;
    }
    for (auto& o : done) {
        if (o.action == "started")
            ctx.out.line("started " + o.name + ", pid " + std::to_string(o.pid) + ", log " + relative(ws, o.log));
        else if (o.action == "running")
            ctx.out.line(o.name + " already runs");
        else if (o.action == "shared")
            ctx.out.line(o.name + " already runs, now shared");
        else
            ctx.out.line("using " + o.name + ", which rant did not start");
    }
}

/* One node of a node type, named after it. Starting the same type again gives the copy a
 * free name, talker_1, then talker_2, since two nodes of one name collide on the mesh. */
static int start_node(app::Context& ctx, const std::string& ref) {
    config::Plan plan = config::plan_node(ctx.cwd, ref);
    require_plan(ctx, plan);
    if (ctx.args.has("dry-run")) {
        print_plan(ctx, plan);
        return 0;
    }
    const config::Instance& inst = plan.instances.front();

    std::set<std::string> taken;
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    for (auto& p : mesh.peers()) taken.insert(p.name);

    run::Session s(*plan.workspace);
    for (auto& i : s.state().instances) taken.insert(i.name);
    std::string name = inst.name;
    for (int n = 1; taken.count(name); n++) name = inst.name + "_" + std::to_string(n);

    state::Root root{ "node", name, {} };
    state::Instance rec = run::spawn(s.workspace(), inst, name);
    rec.roots.push_back(root.key());
    s.state().roots.push_back(root);
    s.state().instances.push_back(rec);
    s.save();
    report(ctx, s.workspace(), { { name, "started", rec.tracking.pid, rec.log } });
    return 0;
}

static void group_help(app::Context& ctx, const std::string& group) {
    config::GroupInfo g = config::describe_group(ctx.cwd, group);
    if (!g.diagnostics.empty()) {
        for (auto& d : g.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    ctx.out.line("Usage: rant start group " + g.name + (g.params.empty() ? "" : " [key=value...]"));
    if (!g.description.empty()) ctx.out.line("\n" + g.description);
    print_params(ctx, g.params);
}

/* A group root: its nodes start in plan order. A node already running for another root
 * with the same settings is shared, and a node of that name on the mesh that rant did not
 * start can stand in after a question. Every check runs before the first node starts. */
static int start_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params) {
    if (ctx.args.has("help")) {
        group_help(ctx, group);
        return 0;
    }
    config::Plan plan = config::plan_group(ctx.cwd, group, params);
    require_plan(ctx, plan);
    if (ctx.args.has("dry-run")) {
        print_plan(ctx, plan);
        return 0;
    }
    state::Root root{ "group", plan.group, plan.params };

    mesh::Client mesh(ctx.domain);
    mesh.settle();
    run::Session s(*plan.workspace);
    auto managed = run::managed_peers(mesh, s.state());

    std::vector<std::string> actions;
    for (auto& inst : plan.instances) {
        if (state::Instance* running = s.state().instance(inst.name)) {
            if (!run::same_spec(*running, inst))
                throw app::Failure("`" + inst.name + "` already runs with other settings for " + running->roots.front() +
                                   ", stop it first or give one of them another name");
            bool mine = std::count(running->roots.begin(), running->roots.end(), root.key()) > 0;
            actions.push_back(mine ? "running" : "shared");
            continue;
        }
        bool external = false;
        for (auto& p : mesh.peers()) external |= p.name == inst.name && !managed.count(p.id);
        if (external) {
            if (!ui::confirm("`" + inst.name + "` already runs on the mesh, but rant did not start it. Use it for " +
                                 plan.group + "?", ctx.yes))
                throw app::Failure("nothing was started");
            actions.push_back("external");
            continue;
        }
        actions.push_back("started");
    }

    if (!s.state().root(root.key())) s.state().roots.push_back(root);
    std::vector<Outcome> done;
    try {
        for (size_t i = 0; i < plan.instances.size(); i++) {
            const config::Instance& inst = plan.instances[i];
            if (actions[i] == "started") {
                state::Instance rec = run::spawn(s.workspace(), inst, inst.name);
                rec.roots.push_back(root.key());
                s.state().instances.push_back(rec);
                done.push_back({ inst.name, "started", rec.tracking.pid, rec.log });
            } else if (actions[i] == "shared" || actions[i] == "running") {
                state::Instance* running = s.state().instance(inst.name);
                if (actions[i] == "shared") running->roots.push_back(root.key());
                done.push_back({ inst.name, actions[i], running->tracking.pid, running->log });
            } else {
                done.push_back({ inst.name, "external", 0, {} });
            }
        }
    } catch (...) {
        s.save();    /* what did start stays tracked */
        report(ctx, s.workspace(), done);
        throw;
    }
    s.save();
    report(ctx, s.workspace(), done);
    return 0;
}

/* The start command line a name alone was most likely meant as. */
static std::string guess(app::Context& ctx, const std::string& name) {
    if (config::describe_group(ctx.cwd, name).diagnostics.empty()) return "did you mean `rant start group " + name + "`?";
    if (config::plan_node(ctx.cwd, name).diagnostics.empty()) return "did you mean `rant start node " + name + "`?";
    return "";
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    bool group = !w.empty() && w[0] == "group";
    if (ctx.args.has("help") && !(group && w.size() >= 2)) {
        ctx.out.line(app::command_help(start()));
        return 0;
    }
    if (w.empty() || (w[0] != "node" && !group)) {
        std::string hint = w.empty() ? "" : guess(ctx, w[0]);
        throw app::UsageError("say what to start: " + (hint.empty() ? std::string("`rant start node <node>` or `rant start group <name>`") : hint));
    }
    if (w.size() < 2) throw app::UsageError("start " + w[0] + " needs a name");
    if (group) return start_group(ctx, w[1], { w.begin() + 2, w.end() });
    if (w.size() != 2) throw app::UsageError("start node takes one node, such as `rant start node talker`");
    return start_node(ctx, w[1]);
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) return { { "node", "group" } };
    if (w[0] == "node" && w.size() == 1) return { r.node_types() };
    if (w[0] != "group") return {};
    if (w.size() == 1) return { r.groups() };
    return { complete::params(r.group(w[1]), r.partial, { w.begin() + 2, w.end() }) };
}

app::Command start() {
    app::Command c{ "start", "node <node> | group <name> [key=value...]",
                    "start nodes, detached, logging to the logs folder", app::Section::Workspace,
                    { { "dry-run", 0, "", "show what would run without starting it" } }, run };
    c.own_help = true;
    c.complete = complete_words;
    return c;
}

}
