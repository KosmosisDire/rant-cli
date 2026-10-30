#include <algorithm>
#include <cstdlib>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/build.hpp"
#include "commands/commands.hpp"
#include "commands/enable.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "commands/summary.hpp"
#include "complete/complete.hpp"
#include "process/supervisor.hpp"
#include "run/nodes.hpp"
#include "services/entry.hpp"
#include "services/services.hpp"
#include "ui/prompt.hpp"
#include "ui/table.hpp"
#include "util/home.hpp"
#include "util/match.hpp"

namespace commands {

using json = nlohmann::ordered_json;

/* The placement variables of the shell, which the plans of the login start need too. */
static std::map<std::string, std::string> placement_env() {
    std::map<std::string, std::string> out;
    for (const char* name : { "RANT_DOMAIN", "RANT_PREFIX", "RANT_NODE_NAME_PREFIX" })
        if (const char* v = std::getenv(name); v && *v) out[name] = v;
    return out;
}

/* What the words name. The name is the shortest that finds it from the folder, where the
 * login start runs it: the group or node alone, else with its package. */
struct Named {
    services::Enabled e;
    std::string       full;    /* the group in full, or the node's mesh name */
};

static Named named(app::Context& ctx, const std::vector<std::string>& w, const std::string& verb) {
    if (w.empty())
        throw app::UsageError("say what to " + verb + ": `rant " + verb + " <group>`, or one node with `rant " + verb + " node <node>`");
    Named n;
    services::Enabled& e = n.e;
    e.folder = ctx.require_scope().root;
    if (w[0] == "node") {
        if (w.size() != 2) throw app::UsageError(verb + " node takes one node, such as `rant " + verb + " node talker`");
        config::Plan plan = config::plan_node(ctx.cwd(), w[1]);
        require_plan(ctx, plan);
        const config::Instance& inst = plan.instances.front();
        std::string bare = inst.type.substr(inst.type.rfind('/') + 1);
        config::Plan short_plan = config::plan_node(e.folder, bare);
        e.kind = "node";
        e.name = short_plan.diagnostics.empty() && short_plan.instances.front().type == inst.type ? bare : inst.type;
        n.full = inst.name;
        return n;
    }
    config::Named what = config::named(ctx.cwd(), w[0]);
    if (!what.group && what.node) throw app::UsageError("`" + w[0] + "` is a node, " + verb + " it with `rant " + verb + " node " + w[0] + "`");
    if (!what.group) throw app::Failure("no group named `" + w[0] + "`, see `rant ls groups`");
    n.full = group_name(ctx, w[0]);
    std::string stem = n.full.substr(n.full.rfind('/') + 1);
    config::GroupInfo g = config::describe_group(e.folder, stem);
    e.kind = "group";
    e.name = g.diagnostics.empty() && g.name == n.full ? stem : n.full;
    // a path counts from here, while the start plans from the folder, so it is kept resolved
    e.words = resolved_params(ctx, w[0], { w.begin() + 1, w.end() });
    return n;
}

/* The entry that starts what is enabled to start at when. */
static services::entry::When entry_for(const std::string& when) {
    return when == "login" ? services::entry::When::Login : services::entry::When::Boot;
}

/* Writes the entry that starts it, the boot one asking for admin the first time, saying so
 * first. */
static void write_entry(app::Context& ctx, const std::string& when) {
    services::entry::When w = entry_for(when);
    if (w == services::entry::When::Boot && !services::entry::present(w))
        ctx.out.note("starting at boot takes admin once, to add " + services::entry::boot_place());
    services::entry::write(w);
}

/* Keeps the group or node for every boot, or login with --login, writes the entry that
 * starts it, and runs it now unless it runs, building what it starts. */
int enable_named(app::Context& ctx, const std::vector<std::string>& w) {
    services::Enabled e = named(ctx, w, "enable").e;
    if (e.kind == "group") {
        config::Plan plan = config::plan_group(ctx.cwd(), w[0], e.words);
        require_plan(ctx, plan);
    }
    e.domain = ctx.domain_option;
    e.env = placement_env();
    e.start = ctx.args.has("login") ? "login" : "boot";
    if (ctx.args.has("dry-run")) return w[0] == "node" ? run_node(ctx, w[1]) : run_group(ctx, w[0], e.words);

    std::vector<services::Enabled> list = services::enabled();
    auto had = std::find_if(list.begin(), list.end(), [&](const services::Enabled& o) { return o.same(e); });
    std::string was = had == list.end() ? "" : had->start;
    if (had == list.end()) list.push_back(e);
    else had->start = e.start;
    write_entry(ctx, e.start);
    services::save(list);
    bool login_left = std::any_of(list.begin(), list.end(), [](const services::Enabled& o) { return o.start == "login"; });
    if (!login_left) services::entry::remove(services::entry::When::Login);

    int code = 0;
    if (e.kind == "group") {
        code = run_group(ctx, w[0], e.words);
    } else {
        std::string name = config::plan_node(ctx.cwd(), w[1]).instances.front().name;
        if (run::snapshot(ctx).instance(name)) ctx.out.note("`" + name + "` already runs");
        else code = run_node(ctx, w[1]);
    }
    std::string when = e.start == "login" ? "when you log in" : "when this computer starts";
    if (was.empty()) ctx.out.note("enabled " + e.shown() + ", it starts " + when);
    else if (was == e.start) ctx.out.note(e.shown() + " was enabled already, it starts " + when);
    else ctx.out.note(e.shown() + " now starts " + when);
    return code;
}

/* Drops the group or node from the boot or login start, and stops it when it runs. The boot
 * entry stays, so enabling again never asks for admin. */
static int disable_run(app::Context& ctx) {
    auto& w = ctx.args.words;
    Named n = named(ctx, w, "disable");
    services::Enabled& e = n.e;
    std::vector<services::Enabled> list = services::enabled(), gone, kept;
    auto here = [&](const services::Enabled& o) { return util::path_key(o.folder) == util::path_key(e.folder); };
    for (auto& o : list) {
        bool match = o.kind == e.kind && o.name == e.name && (e.words.empty() || o.words == e.words);
        (match && here(o) ? gone : kept).push_back(o);
    }
    if (gone.empty()) {
        for (auto& o : list)
            if (o.kind == e.kind && o.name == e.name)
                throw app::Failure(o.shown() + " is enabled in " + config::to_utf8(o.folder) + ", disable it there");
        throw app::Failure(e.shown() + " is not enabled here, see `rant ls services`");
    }
    if (gone.size() > 1) {
        std::string all;
        for (auto& o : gone) all += (all.empty() ? "" : ", ") + o.shown();
        ctx.out.note(e.shown() + " is enabled " + std::to_string(gone.size()) + " times: " + all);
        if (!ui::confirm("Disable all of them?", ctx.yes)) return 1;
    }
    services::save(kept);
    if (std::none_of(kept.begin(), kept.end(), [](const services::Enabled& o) { return o.start == "login"; }))
        services::entry::remove(services::entry::When::Login);

    int code = 0;
    state::State st = run::snapshot(ctx);
    if (e.kind == "group") {
        bool runs = std::any_of(st.roots.begin(), st.roots.end(), [&](const state::Root& r) { return r.kind == "group" && r.name == n.full; });
        if (runs) code = stop_group(ctx, n.full, e.words);
    } else if (st.instance(n.full)) {
        code = stop_node(ctx, n.full);
    }
    for (auto& o : gone) ctx.out.note("disabled " + o.shown() + ", it no longer starts " + (o.start == "login" ? "when you log in" : "when this computer starts"));
    return code;
}

/* Where an enabled group or node stands: its nodes now, found by planning it as rant run
 * would, and the restarts the service process made. */
struct Standing {
    std::string           state;    /* "running", "stopped", "2 of 3 running", or what broke */
    int                   restarts = 0;
    json                  nodes = json::array();
};

/* An enabled group or node planned in its folder as rant run would, with the key of the root it
 * runs under, empty when it no longer plans. */
struct Planned {
    config::Plan plan;
    std::string  key;
};

static Planned planned(const services::Enabled& e) {
    Planned p{ e.kind == "group" ? config::plan_group(e.folder, e.name, e.words) : config::plan_node(e.folder, e.name), "" };
    if (p.plan.diagnostics.empty())
        p.key = e.kind == "group" ? state::Root{ "group", p.plan.group, p.plan.params }.key() : "node " + p.plan.instances.front().name;
    return p;
}

Kept kept_services(const state::State& st, const config::fs::path& data) {
    Kept k;
    for (auto& e : services::enabled()) {
        Planned p = planned(e);
        if (p.key.empty() || util::path_key(p.plan.workspace->data) != util::path_key(data)) continue;
        k.roots.insert(p.key);
        bool runs = false;
        for (auto& i : st.instances)
            if (std::count(i.roots.begin(), i.roots.end(), p.key)) {
                k.nodes.insert(i.name);
                runs = true;
            }
        if (runs) k.running.push_back(e.shown());
    }
    return k;
}

static Standing standing(const services::Enabled& e) {
    Standing out;
    Planned p = planned(e);
    if (p.key.empty()) {
        out.state = p.plan.diagnostics.front().message;
        return out;
    }
    const std::string& key = p.key;
    state::Store store(p.plan.workspace->data);
    state::prune(store.state(), services::running_in);
    store.save();
    size_t up = 0, all = 0, waiting = 0;
    for (auto& i : store.state().instances) {
        if (!std::count(i.roots.begin(), i.roots.end(), key)) continue;
        bool alive = process::alive(i.tracking);
        all++;
        up += alive;
        waiting += !alive && i.restart != "never";
        out.restarts += i.restarts;
        out.nodes.push_back({ { "name", i.name }, { "running", alive }, { "restart", i.restart }, { "restarts", i.restarts } });
    }
    out.state = all == 0 ? "stopped" : up == all ? "running" : std::to_string(up) + " of " + std::to_string(all) + " running";
    if (waiting) out.state += ", " + std::to_string(waiting) + " restarting";
    return out;
}

int list_services(app::Context& ctx, const std::string& pattern) {
    auto dim = [&](const std::string& s) { return ctx.out.paint(ui::Style::Dim, s); };
    json out = json::array();
    ui::Table t;
    std::vector<services::Enabled> list = services::enabled();
    for (auto& e : list) {
        if (!util::name_matches(pattern, e.name)) continue;
        Standing s = standing(e);
        json j = { { "service", e.shown() }, { "start", e.start }, { "kind", e.kind }, { "name", e.name }, { "params", e.words },
                   { "folder", config::to_utf8(e.folder) }, { "state", s.state }, { "restarts", s.restarts }, { "nodes", s.nodes } };
        if (e.domain) j["domain"] = *e.domain;
        out.push_back(j);
        std::string restarts = s.restarts == 1 ? "1 restart" : s.restarts > 1 ? std::to_string(s.restarts) + " restarts" : "";
        t.row({ e.shown(), dim("at " + e.start), s.state, dim(restarts), dim(ctx.shown(e.folder)) });
    }
    if (ctx.json) {
        ctx.out.line(out.dump(2));
        return 0;
    }
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "SERVICES"));
    for (auto& l : t.lines()) ctx.out.line(l);
    if (t.lines().empty()) ctx.out.line(ctx.out.paint(ui::Style::Faint, "  (none)"));
    for (const char* when : { "boot", "login" }) {
        bool some = std::any_of(list.begin(), list.end(), [&](const services::Enabled& e) { return e.start == when; });
        if (some && !services::entry::present(entry_for(when)))
            ctx.out.note(std::string("nothing starts them at ") + when + " for this rant and home, `rant enable` sets that up again");
    }
    return 0;
}

static complete::Candidates complete_enable(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) {
        std::vector<std::string> out = r.groups();
        out.push_back("node");
        return { out };
    }
    if (w[0] == "node") return { w.size() == 1 ? r.node_types() : std::vector<std::string>{} };
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

/* What is enabled, groups by their name and nodes after the word node. */
static complete::Candidates complete_disable(complete::Request& r) {
    auto& w = r.words;
    bool node = !w.empty() && w[0] == "node";
    if (w.size() > (node ? 1u : 0u)) return {};
    std::vector<std::string> out;
    try {
        for (auto& e : services::enabled())
            if ((e.kind == "node") == node && std::find(out.begin(), out.end(), e.name) == out.end()) out.push_back(e.name);
    } catch (const app::Failure&) {
    }
    if (!node) out.push_back("node");
    return { out };
}

app::Command enable() {
    app::Command c{ "enable", "<group> [key=value...] | node <node>",
                    "run a group, or one node, now and every time this computer starts, which takes admin once",
                    app::Section::Workspace,
                    { { "login", 0, "", "start it when you log in instead, for what needs your desktop" },
                      { "dry-run", 0, "", "show what would run without starting or keeping it" }, no_build_option(), wait_option(),
                      watch_option() },
                    [](app::Context& ctx) { return enable_named(ctx, ctx.args.words); } };
    c.complete = complete_enable;
    return c;
}

app::Command disable() {
    app::Command c{ "disable", "<group> [key=value...] | node <node>", "stop what rant enable named and no longer start it",
                    app::Section::Workspace, {}, disable_run };
    c.complete = complete_disable;
    return c;
}

}
