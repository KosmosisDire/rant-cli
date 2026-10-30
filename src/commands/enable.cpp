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

static bool same_folder(const config::fs::path& a, const config::fs::path& b) { return util::path_key(a) == util::path_key(b); }

/* What messages call the scope whose services enable and disable act on. */
static std::string scope_name(const config::Workspace& ws) { return ws.home ? "rant's home" : "this workspace"; }

static std::string when_text(const std::string& start) { return start == "login" ? "when you log in" : "when this computer starts"; }

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

/* Saves the list, and drops the login entry once nothing starts at login. The boot entry
 * stays, so enabling again never asks for admin. */
static void keep(const std::vector<services::Enabled>& list) {
    services::save(list);
    if (std::none_of(list.begin(), list.end(), [](const services::Enabled& o) { return o.start == "login"; }))
        services::entry::remove(services::entry::When::Login);
}

/* Enables the scope on this machine with the start, domain and placement of this command, and
 * writes the entry that starts it. Says how that changed, unless only a service was added to
 * a scope enabled the same way. */
static void enable_scope(app::Context& ctx, const config::Workspace& ws, bool bare) {
    services::Enabled e{ ws.root, ctx.args.has("login") ? "login" : "boot", ctx.domain_option, placement_env() };
    std::vector<services::Enabled> list = services::enabled();
    auto had = std::find_if(list.begin(), list.end(), [&](const services::Enabled& o) { return same_folder(o.folder, ws.root); });
    std::string was = had == list.end() ? "" : had->start;
    if (had == list.end()) list.push_back(e);
    else *had = e;
    write_entry(ctx, e.start);
    keep(list);
    std::string when = when_text(e.start);
    if (was.empty()) ctx.out.note("enabled " + scope_name(ws) + ", its services start " + when);
    else if (was != e.start) ctx.out.note(scope_name(ws) + " now starts its services " + when);
    else if (bare) ctx.out.note(scope_name(ws) + " was enabled already, its services start " + when);
}

int enable_named(app::Context& ctx, const std::vector<std::string>& w) {
    if (w.empty()) {
        config::Workspace ws = ctx.require_scope(true);    /* a copy, since running enters its root */
        if (ws.services.empty())
            throw app::Failure("no services in " + ctx.shown(ws.services_file) + ", `rant enable <group>` adds one");
        if (!ctx.args.has("dry-run")) enable_scope(ctx, ws, true);
        return run_services(ctx);
    }
    if (w[0] == "node") throw app::UsageError("a service is a group, and `rant new group` makes one for a lone node");
    config::Named what = config::named(ctx.cwd(), w[0]);
    if (!what.group && what.node)
        throw app::UsageError("`" + w[0] + "` is a node and a service is a group, `rant new group` makes one for it");
    if (!what.group) throw app::Failure("no group named `" + w[0] + "`, see `rant ls groups`");
    std::vector<std::string> params(w.begin() + 1, w.end());
    require_plan(ctx, config::plan_group(ctx.cwd(), w[0], params));
    if (ctx.args.has("dry-run")) return run_group(ctx, w[0], params);

    config::Edit edit = config::add_service(ctx.cwd(), w[0], params);
    if (!edit.diagnostics.empty()) {
        for (auto& d : edit.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    config::Workspace ws = ctx.require_scope();
    enable_scope(ctx, ws, false);
    int code = run_group(ctx, w[0], params);
    std::string shown = w[0];
    for (auto& p : params) shown += " " + p;
    std::string file = ctx.shown(ws.services_file);
    ctx.out.note(edit.changed ? "added " + shown + " to the services in " + file : shown + " is a service in " + file + " already");
    return code;
}

/* A service planned in the folder of its file as rant run would, with the key of the root it
 * runs under, empty with the problem when it does not plan. */
struct Planned {
    config::Plan plan;
    std::string  key;
    std::string  problem;
};

static Planned planned(const config::Workspace& ws, const config::Service& s) {
    Planned p;
    if (s.problem) {
        p.problem = s.problem->str();
        return p;
    }
    p.plan = config::plan_group(ws.root, s.full, s.words);
    if (!p.plan.diagnostics.empty()) p.problem = p.plan.diagnostics.front().str();
    else p.key = state::Root{ "group", p.plan.group, p.plan.params }.key();
    return p;
}

/* The workspace or rant's home whose root is folder, with its services, and any error met
 * reading it. No workspace when folder is no longer one. */
static config::Opened scope_at(const config::fs::path& folder) {
    config::Opened o = config::open(folder, true);
    if (o.workspace && !same_folder(o.workspace->root, folder)) o.workspace.reset();
    return o;
}

/* Stops the runs of these services of the scope that run, each run alone. */
static int stop_services(app::Context& ctx, const config::Workspace& ws, const std::vector<size_t>& which) {
    ctx.enter(ws.root);
    state::State st = run::snapshot(ctx);
    int code = 0;
    for (size_t i : which) {
        Planned p = planned(ws, ws.services[i]);
        if (p.key.empty() || std::none_of(st.roots.begin(), st.roots.end(), [&](const state::Root& r) { return r.key() == p.key; }))
            continue;
        std::vector<std::string> params;
        for (auto& [k, v] : p.plan.params) params.push_back(k + "=" + v);
        int c = stop_group(ctx, p.plan.group, params);
        if (!code) code = c;
    }
    return code;
}

/* With no words, stops the scope's services and drops its boot or login start, its blocks
 * kept. With a group, removes the blocks of it, the one the params pick when given, and stops
 * those runs. */
static int disable_run(app::Context& ctx) {
    auto& w = ctx.args.words;
    config::Workspace ws = ctx.require_scope(true);    /* a copy, since stopping enters its root */
    std::vector<size_t> all(ws.services.size());
    for (size_t i = 0; i < all.size(); i++) all[i] = i;
    if (w.empty()) {
        std::vector<services::Enabled> list = services::enabled(), kept;
        std::string start;
        for (auto& o : list) {
            if (same_folder(o.folder, ws.root)) start = o.start;
            else kept.push_back(o);
        }
        if (start.empty()) throw app::Failure(scope_name(ws) + " is not enabled, see `rant ls services`");
        keep(kept);
        int code = stop_services(ctx, ws, all);
        ctx.out.note("disabled " + scope_name(ws) + ", its services no longer start " + when_text(start));
        return code;
    }
    if (w[0] == "node") throw app::UsageError("a service is a group, such as `rant disable Bridge`");
    std::string full = group_name(ctx, w[0]);
    std::vector<std::string> params(w.begin() + 1, w.end());
    std::string key;
    if (!params.empty()) {
        config::Plan plan = config::plan_group(ctx.cwd(), w[0], params);
        require_plan(ctx, plan);
        key = state::Root{ "group", plan.group, plan.params }.key();
    }
    auto of_group = [&](const config::Service& s) { return s.full.empty() ? config::same_name(s.group, w[0]) : s.full == full; };
    std::vector<size_t> gone;
    for (size_t i : all)
        if (of_group(ws.services[i]) && (key.empty() || planned(ws, ws.services[i]).key == key)) gone.push_back(i);
    std::string shown = w[0];
    for (auto& p : params) shown += " " + p;
    if (gone.empty()) {
        for (auto& o : services::enabled()) {
            if (same_folder(o.folder, ws.root)) continue;
            config::Opened other = scope_at(o.folder);
            if (other.workspace && std::any_of(other.workspace->services.begin(), other.workspace->services.end(),
                                               [&](const config::Service& s) { return config::same_name(s.group, w[0]); }))
                throw app::Failure("`" + w[0] + "` is a service of " + ctx.shown(o.folder) + ", disable it there");
        }
        throw app::Failure("`" + shown + "` is not a service here, see `rant ls services`");
    }
    config::Edit edit = config::remove_services(ws.root, gone);
    if (!edit.diagnostics.empty()) {
        for (auto& d : edit.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    int code = stop_services(ctx, ws, gone);
    for (size_t i : gone) ctx.out.note("removed " + ws.services[i].shown() + " from the services in " + ctx.shown(ws.services_file));
    return code;
}

Kept kept_services(const state::State& st, const config::fs::path& data) {
    Kept k;
    config::fs::path folder = state::folder_of(data);
    std::vector<services::Enabled> list = services::enabled();
    if (std::none_of(list.begin(), list.end(), [&](const services::Enabled& e) { return same_folder(e.folder, folder); })) return k;
    config::Opened o = scope_at(folder);
    if (!o.workspace) return k;
    for (auto& s : o.workspace->services) {
        Planned p = planned(*o.workspace, s);
        if (p.key.empty()) continue;
        k.roots.insert(p.key);
        bool runs = false;
        for (auto& i : st.instances)
            if (std::count(i.roots.begin(), i.roots.end(), p.key)) {
                k.nodes.insert(i.name);
                runs = true;
            }
        if (runs) k.running.push_back(s.shown());
    }
    return k;
}

/* Where a service stands: its nodes now, found by planning it as rant run would, and the
 * restarts the service process made. */
struct Standing {
    std::string state;    /* "running", "stopped", "2 of 3 running", or what broke */
    int         restarts = 0;
    json        nodes = json::array();
};

static Standing standing(const config::Workspace& ws, const config::Service& s) {
    Standing out;
    Planned p = planned(ws, s);
    if (p.key.empty()) {
        out.state = p.problem;
        return out;
    }
    state::Store store(ws.data);
    state::prune(store.state(), services::running_in);
    store.save();
    size_t up = 0, all = 0, waiting = 0;
    for (auto& i : store.state().instances) {
        if (!std::count(i.roots.begin(), i.roots.end(), p.key)) continue;
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
    auto faint = [&](const std::string& s) { return ctx.out.paint(ui::Style::Faint, s); };
    std::vector<services::Enabled> list = services::enabled();
    /* every enabled folder, then this scope while it is not enabled, so what rant enable would
     * start shows */
    std::vector<std::pair<config::fs::path, std::optional<std::string>>> folders;
    for (auto& e : list) folders.push_back({ e.folder, e.start });
    if (const config::Workspace* here = ctx.scope())
        if (std::none_of(list.begin(), list.end(), [&](const services::Enabled& e) { return same_folder(e.folder, here->root); }))
            folders.push_back({ here->root, std::nullopt });

    json out = json::array();
    ui::Table t;
    for (auto& [folder, start] : folders) {
        std::string at = start ? "at " + *start : "not enabled";
        config::Opened o = scope_at(folder);
        if (!o.workspace) {
            std::string why = o.diagnostics.empty() ? "no workspace there now" : o.diagnostics.front().str();
            t.row({ faint("(none)"), dim(at), why, "", dim(ctx.shown(folder)) });
            continue;
        }
        if (o.workspace->services.empty() && pattern.empty()) t.row({ faint("(none)"), dim(at), "", "", dim(ctx.shown(folder)) });
        for (auto& s : o.workspace->services) {
            if (!util::name_matches(pattern, s.group)) continue;
            Standing st = standing(*o.workspace, s);
            json j = { { "service", s.shown() }, { "group", s.full.empty() ? s.group : s.full }, { "params", s.words },
                       { "folder", config::to_utf8(folder) }, { "start", start ? json(*start) : json(nullptr) },
                       { "state", st.state }, { "restarts", st.restarts }, { "nodes", st.nodes } };
            out.push_back(j);
            std::string restarts = st.restarts == 1 ? "1 restart" : st.restarts > 1 ? std::to_string(st.restarts) + " restarts" : "";
            t.row({ s.shown(), dim(at), st.state, dim(restarts), dim(ctx.shown(folder)) });
        }
    }
    if (ctx.json) {
        ctx.out.line(out.dump(2));
        return 0;
    }
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "SERVICES"));
    for (auto& l : t.lines()) ctx.out.line(l);
    if (t.lines().empty()) ctx.out.line(faint("  (none)"));
    for (const char* when : { "boot", "login" }) {
        bool some = std::any_of(list.begin(), list.end(), [&](const services::Enabled& e) { return e.start == when; });
        if (some && !services::entry::present(entry_for(when)))
            ctx.out.note(std::string("nothing starts them at ") + when + " for this rant and home, `rant enable` sets that up again");
    }
    return 0;
}

static complete::Candidates complete_enable(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) return { r.groups() };
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

/* The groups the scope's services name, then the params of the one given. */
static complete::Candidates complete_disable(complete::Request& r) {
    auto& w = r.words;
    if (w.empty()) return { r.services() };
    return { complete::params(r.group(w[0]), r.partial, { w.begin() + 1, w.end() }) };
}

app::Command enable() {
    app::Command c{ "enable", "[<group> [key=value...]]",
                    "run the services now and at every boot, which takes admin once, a group named joining them first",
                    app::Section::Workspace,
                    { { "login", 0, "", "start them when you log in instead, for what needs your desktop" },
                      { "dry-run", 0, "", "show what would run without starting or keeping it" }, no_build_option(), wait_option(),
                      watch_option() },
                    [](app::Context& ctx) { return enable_named(ctx, ctx.args.words); } };
    c.complete = complete_enable;
    return c;
}

app::Command disable() {
    app::Command c{ "disable", "[<group> [key=value...]]",
                    "stop the services and no longer start them, or take one group off them",
                    app::Section::Workspace, {}, disable_run };
    c.complete = complete_disable;
    return c;
}

}
