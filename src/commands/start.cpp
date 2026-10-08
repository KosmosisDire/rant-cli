#include <set>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "run/nodes.hpp"

namespace commands {

using json = nlohmann::ordered_json;

static std::string relative(const config::Workspace& ws, const std::string& path) {
    std::error_code ec;
    auto rel = config::fs::relative(config::from_utf8(path), ws.root, ec);
    return ec || rel.empty() ? path : config::to_utf8(rel);
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

    if (ctx.json) {
        json started = json::array();
        started.push_back({ { "name", name }, { "pid", rec.tracking.pid }, { "log", rec.log } });
        ctx.out.line(json{ { "started", started } }.dump(2));
    } else {
        ctx.out.line("started " + name + ", pid " + std::to_string(rec.tracking.pid) + ", log " +
                     relative(s.workspace(), rec.log));
    }
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty() || (w[0] != "node" && w[0] != "group"))
        throw app::UsageError("say what to start: `rant start node <node type>` or `rant start group <name>`");
    if (w[0] == "group") throw app::Failure("groups are not supported yet");
    if (w.size() != 2) throw app::UsageError("start node takes one node type, such as `rant start node talker`");
    return start_node(ctx, w[1]);
}

app::Command start() {
    return { "start", "node <node type> | group <name> [key=value...]", "start nodes, detached, logging to the logs folder",
             app::Section::Workspace,
             { { "dry-run", 0, "", "show what would run without starting it" } }, run };
}

}
