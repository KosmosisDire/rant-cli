#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "run/nodes.hpp"
#include "ui/prompt.hpp"

namespace commands {

static std::string list(const std::vector<std::string>& v) {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : ", ") + x;
    return s;
}

static void report(app::Context& ctx, const std::string& name, process::Stopped how) {
    switch (how) {
    case process::Stopped::Gracefully:  ctx.out.line("stopped " + name); break;
    case process::Stopped::Killed:
        ctx.out.line("stopped " + name + ", killed after " + std::to_string(run::grace.count()) + " s");
        break;
    case process::Stopped::AlreadyGone: ctx.out.line(name + " had already exited"); break;
    }
}

/* Stops one node rant started, whichever root asked for it. */
static int stop_node(app::Context& ctx, const std::string& name) {
    run::Session s(ctx.require_workspace());
    state::State& st = s.state();
    state::Instance* inst = st.instance(name);
    if (!inst) {
        s.save();
        mesh::Client mesh(ctx.domain);
        mesh.settle();
        for (auto& p : mesh.peers())
            if (p.name == name)
                throw app::Failure("`" + name + "` runs, but rant did not start it in this workspace, so it will not stop it");
        throw app::Failure("no node named `" + name + "` is running, see `rant ls`");
    }

    std::vector<std::string> groups;
    for (auto& key : inst->roots)
        if (key.rfind("group ", 0) == 0) groups.push_back(key.substr(6));
    if (!groups.empty() && !ui::confirm("`" + name + "` belongs to the running group " + list(groups) + ". Stop it anyway?", ctx.yes))
        return 1;

    process::Stopped how = run::halt(*inst);
    st.instances.erase(std::remove_if(st.instances.begin(), st.instances.end(),
                                      [&](const state::Instance& i) { return i.name == name; }),
                       st.instances.end());
    st.roots.erase(std::remove_if(st.roots.begin(), st.roots.end(),
                                  [&](const state::Root& r) { return r.kind == "node" && r.name == name; }),
                   st.roots.end());
    s.save();
    report(ctx, name, how);
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() != 2 || (w[0] != "node" && w[0] != "group"))
        throw app::UsageError("say what to stop: `rant stop node <name>` or `rant stop group <name>`");
    if (w[0] == "group") throw app::Failure("groups are not supported yet");
    return stop_node(ctx, w[1]);
}

app::Command stop() {
    return { "stop", "node <name> | group <name> [key=value...]", "stop what start started", app::Section::Workspace,
             {}, run };
}

}
