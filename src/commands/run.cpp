#include <algorithm>
#include <cctype>
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
#include "services/services.hpp"
#include "ui/checklist.hpp"
#include "ui/prompt.hpp"
#include "ui/terminal.hpp"
#include "util/home.hpp"
#include "util/text.hpp"

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
    int code = bring_up(ctx, summary, *plan.workspace, { rec }, "", "started");
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
    int code = bring_up(ctx, summary, *plan.workspace, started, plan.group, "started");
    return built ? code : 1;
}

std::string group_name(app::Context& ctx, const std::string& group) {
    config::GroupInfo info = config::describe_group(ctx.cwd(), group);
    return info.diagnostics.empty() ? info.name : group;
}

/* A running group's root is the name, in full or by its stem. */
static bool is_group_of(const state::Root& r, const std::string& name) {
    return r.kind == "group" && (config::same_name(r.name, name) || config::same_name(r.name.substr(r.name.rfind('/') + 1), name));
}

bool names_group(app::Context& ctx, const std::string& name) {
    if (!ctx.scope()) return false;
    state::State st = run::snapshot(ctx);
    for (auto& r : st.roots)
        if (is_group_of(r, name)) return true;
    if (st.instance(name)) return false;
    return config::named(ctx.cwd(), name).group;
}

std::vector<std::pair<config::fs::path, state::State>> states_anywhere(app::Context& ctx, bool prune) {
    std::vector<config::fs::path> dirs = state::remembered();
    std::sort(dirs.begin(), dirs.end());
    if (const config::Workspace* here = ctx.scope()) dirs.insert(dirs.begin(), here->data);
    std::vector<std::pair<config::fs::path, state::State>> out;
    std::set<std::string> seen;
    for (auto& d : dirs) {
        if (!seen.insert(util::path_key(d)).second) continue;
        /* read and let go at once, so no other command waits on it */
        state::Store store(d, prune);
        if (prune) {
            state::prune(store.state(), services::running_in);
            store.save();
        }
        out.push_back({ state::folder_of(d), store.state() });
    }
    return out;
}

std::optional<std::vector<config::fs::path>> where_running(app::Context& ctx, const std::string& name, bool node,
                                                           const std::string& verb) {
    const config::Workspace* here = ctx.scope();
    struct Found {
        config::fs::path         root;
        std::vector<std::string> groups, nodes;
    };
    std::vector<Found> found;
    for (auto& [root, st] : states_anywhere(ctx, true)) {
        Found f{ root, {}, {} };
        for (auto& r : st.roots)
            if (!node && is_group_of(r, name)) f.groups.push_back(r.key().substr(6));
        for (auto& i : st.instances)
            if (config::same_name(i.name, name)) f.nodes.push_back(i.name);
        if (!f.groups.empty() || !f.nodes.empty()) found.push_back(f);
    }
    /* a group of it running anywhere wins over a node of it */
    bool groups = std::any_of(found.begin(), found.end(), [](const Found& f) { return !f.groups.empty(); });
    std::vector<Found> runs;
    for (auto& f : found)
        if (groups ? !f.groups.empty() : !f.nodes.empty()) runs.push_back(f);
    std::vector<config::fs::path> out;
    if (runs.size() <= 1) {
        for (auto& f : runs) out.push_back(f.root);
        return out;
    }
    std::vector<std::string> where;
    std::vector<ui::Choice> choices;
    for (auto& f : runs) {
        where.push_back(ctx.shown(f.root));
        bool current = here && util::path_key(f.root) == util::path_key(here->root);
        choices.push_back({ ctx.shown(f.root), util::joined(groups ? f.groups : f.nodes), current });
    }
    if (ctx.yes || !ui::is_terminal(stdin))
        throw app::Failure("`" + name + "` runs in " + std::to_string(runs.size()) + " workspaces: " + util::joined(where) +
                           ", pick one in a terminal");
    std::string title = verb + " `" + name + "` in";
    title[0] = (char)std::toupper((unsigned char)title[0]);
    std::optional<std::vector<size_t>> ticked = ui::pick(ctx.out, title, choices);
    if (!ticked) return std::nullopt;
    for (size_t i : *ticked) out.push_back(runs[i].root);
    return out;
}

int in_each(app::Context& ctx, const std::vector<config::fs::path>& roots, const std::function<int(const std::string&)>& act) {
    const config::Workspace* here = ctx.scope();
    std::string own = here ? util::path_key(here->root) : "";
    int code = 0;
    for (auto& root : roots) {
        std::string in = util::path_key(root) == own ? "" : ctx.shown(root);
        ctx.enter(root);
        int c = act(in);
        if (!code) code = c;
    }
    return code;
}

std::vector<std::string> resolved_params(app::Context& ctx, const std::string& group, std::vector<std::string> words) {
    if (words.empty()) return words;
    config::Plan plan = config::plan_group(ctx.cwd(), group, words);
    if (!plan.diagnostics.empty()) return words;
    config::GroupInfo g = config::describe_group(ctx.cwd(), group);
    for (auto& word : words) {
        std::string key = word.substr(0, word.find('='));
        for (auto& p : g.params)
            if (p.name == key && p.type == "path") word = key + "=" + plan.params[key];
    }
    return words;
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
