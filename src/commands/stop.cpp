#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "commands/plan_output.hpp"
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

/* The group roots to stop: the one root params name, else every root of the group, which
 * takes a yes when there are several. The group's own name is looked up, so a bare name
 * finds a group in a package. */
static std::vector<state::Root> targets(app::Context& ctx, run::Session& s, const std::string& group,
                                        const std::vector<std::string>& params) {
    if (!params.empty()) {
        config::Plan plan = config::plan_group(ctx.cwd, group, params);
        require_plan(ctx, plan);
        state::Root root{ "group", plan.group, plan.params };
        if (!s.state().root(root.key())) throw app::Failure("`" + root.key().substr(6) + "` is not running, see `rant ls`");
        return { root };
    }
    config::GroupInfo info = config::describe_group(ctx.cwd, group);
    std::string name = info.diagnostics.empty() ? info.name : group;
    std::vector<state::Root> out;
    for (auto& r : s.state().roots)
        if (r.kind == "group" && r.name == name) out.push_back(r);
    if (out.empty()) throw app::Failure("group `" + name + "` is not running, see `rant ls`");
    if (out.size() > 1) {
        std::vector<std::string> keys;
        for (auto& r : out) keys.push_back(r.key().substr(6));
        ctx.out.note("group `" + name + "` runs " + std::to_string(out.size()) + " times: " + list(keys));
        if (!ui::confirm("Stop all of them?", ctx.yes)) return {};
    }
    return out;
}

static int stop_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params) {
    run::Session s(ctx.require_workspace());
    auto roots = targets(ctx, s, group, params);
    if (roots.empty()) return 1;
    for (auto& r : roots) {
        for (auto& rel : run::release_root(s, r.key())) {
            if (rel.kept_for.empty()) report(ctx, rel.name, rel.how);
            else ctx.out.line("kept " + rel.name + ", still needed by " + list(rel.kept_for));
        }
        s.save();
    }
    return 0;
}

/* The stop command line a name alone was most likely meant as. */
static std::string guess(app::Context& ctx, const std::string& name) {
    if (config::describe_group(ctx.cwd, name).diagnostics.empty()) return "did you mean `rant stop group " + name + "`?";
    if (run::snapshot(ctx).instance(name)) return "did you mean `rant stop node " + name + "`?";
    return "";
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() < 2 || (w[0] != "node" && w[0] != "group")) {
        std::string hint = w.size() == 1 && w[0] != "node" && w[0] != "group" ? guess(ctx, w[0]) : "";
        throw app::UsageError("say what to stop: " + (hint.empty() ? std::string("`rant stop node <name>` or `rant stop group <name>`") : hint));
    }
    if (w[0] == "group") return stop_group(ctx, w[1], { w.begin() + 2, w.end() });
    if (w.size() != 2) throw app::UsageError("stop node takes one name");
    return stop_node(ctx, w[1]);
}

static complete::Candidates complete_words(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) return { { "node", "group" } };
    if (w[0] == "node" && w.size() == 1) return { r.running_nodes() };
    if (w[0] != "group") return {};
    if (w.size() == 1) return { r.running_groups() };
    return { complete::params(r.group(w[1]), r.partial, { w.begin() + 2, w.end() }) };
}

app::Command stop() {
    app::Command c{ "stop", "node <name> | group <name> [key=value...]", "stop what start started",
                    app::Section::Workspace, {}, run };
    c.complete = complete_words;
    return c;
}

}
