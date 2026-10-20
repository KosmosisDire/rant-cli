#include <algorithm>
#include <cstdio>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/build.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "complete/complete.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "ui/table.hpp"

namespace commands {

using json = nlohmann::ordered_json;

static std::string shown_path(const config::Workspace& ws, const config::fs::path& p) {
    std::error_code ec;
    auto rel = config::fs::relative(p, ws.root, ec);
    return config::to_utf8(ec || rel.empty() ? p : rel);
}

static void require(app::Context& ctx, const std::vector<config::Diagnostic>& diags) {
    if (diags.empty()) return;
    for (auto& d : diags) ctx.out.error(d.str());
    throw app::Failure("");
}

static json node_types_json(const std::vector<config::NodeType>& types) {
    json nodes = json::array();
    for (auto& n : types)
        nodes.push_back({ { "name", n.name },
                          { "kind", config::kind_name(n.kind) },
                          { "path", n.path ? json(config::to_utf8(*n.path)) : json(nullptr) },
                          { "run", n.run },
                          { "cwd", config::to_utf8(n.cwd) } });
    return nodes;
}

static json dry_run_json(const config::Workspace& ws, const config::Build& b) {
    json packages = json::array();
    for (auto& p : ws.packages)
        if (!p.installed) packages.push_back({ { "name", p.name }, { "dir", config::to_utf8(p.dir) }, { "nodes", node_types_json(p.nodes) } });
    json edges = json::array();
    for (auto& e : b.edges) edges.push_back({ { "from", e.from }, { "to", e.to }, { "source", e.source } });
    json steps = json::array();
    for (auto& s : b.steps) {
        json step = { { "package", s.package }, { "dir", config::to_utf8(s.dir) } };
        bool configures = !s.configure.empty() && (s.unconfigured || !s.moved_from.empty());
        step["configure"] = configures ? json(s.configure) : json(nullptr);
        step["moved_from"] = s.moved_from.empty() ? json(nullptr) : json(config::to_utf8(s.moved_from));
        json clean = json::array();
        for (auto& c : s.clean) clean.push_back(config::to_utf8(c));
        step["clean"] = clean;
        step["commands"] = s.commands;
        steps.push_back(step);
    }
    return { { "workspace", config::to_utf8(ws.root) },
             { "packages", packages },
             { "loose", node_types_json(ws.loose) },
             { "dependencies", edges },
             { "build", steps } };
}

/* Everything a build would do, and why: the workspace's packages, their node types, every
 * dependency with where it was found, the order and the exact commands. */
static void dry_run(app::Context& ctx, const config::Workspace& ws, const config::Build& b) {
    if (ctx.json) {
        ctx.out.line(dry_run_json(ws, b).dump(2));
        return;
    }
    std::vector<const config::Package*> sorted;
    for (auto& p : ws.packages)
        if (!p.installed) sorted.push_back(&p);
    std::sort(sorted.begin(), sorted.end(), [](auto* a, auto* b) { return a->name < b->name; });
    ui::Table packages, nodes, edges;
    auto node_row = [&](const config::NodeType& n) {
        nodes.row({ n.ref(), ctx.out.paint(ui::Style::Dim, config::kind_name(n.kind)),
                    n.path ? shown_path(ws, *n.path) : process::shown(n.run) });
    };
    for (auto* p : sorted) {
        packages.row({ p->name, ctx.out.paint(ui::Style::Dim, shown_path(ws, p->dir)) });
        for (auto& n : p->nodes) node_row(n);
    }
    for (auto& n : ws.loose) node_row(n);
    for (auto& e : b.edges) edges.row({ e.from + " -> " + e.to, ctx.out.paint(ui::Style::Dim, e.source) });

    auto section = [&](const char* title, const ui::Table& t) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, title));
        for (auto& l : t.lines()) ctx.out.line(l);
        if (t.empty()) ctx.out.line(ctx.out.paint(ui::Style::Dim, "  (none)"));
        ctx.out.line();
    };
    section("PACKAGES", packages);
    section("NODES", nodes);
    section("DEPENDENCIES", edges);

    ctx.out.line(ctx.out.paint(ui::Style::Bold, "BUILD ORDER"));
    bool clean = ctx.args.has("clean");
    auto dim = [&](const std::string& t) { return ctx.out.paint(ui::Style::Dim, t); };
    int n = 0;
    for (auto& s : b.steps) {
        ctx.out.line("  " + std::to_string(++n) + ". " + s.package);
        bool moved = !clean && !s.moved_from.empty();
        if (clean)
            for (auto& c : s.clean) ctx.out.line("     remove " + shown_path(ws, c));
        if (moved)
            ctx.out.line("     remove " + shown_path(ws, s.build_dir) + dim("  (made in " + config::to_utf8(s.moved_from) + ", asks first)"));
        if (!s.configure.empty() && (clean || moved || s.unconfigured))
            ctx.out.line("     " + process::shown(s.configure) + (clean || moved ? "" : dim("  (asks first)")));
        for (auto& c : s.commands) ctx.out.line("     " + process::shown(c));
        if (s.configure.empty() && s.commands.empty()) ctx.out.line(ctx.out.paint(ui::Style::Dim, "     nothing to build"));
    }
    if (b.steps.empty()) ctx.out.line(ctx.out.paint(ui::Style::Dim, "  (none)"));
}

/* Runs one command in a package, inheriting the terminal. A failure names both. */
static void run_command(const config::BuildStep& s, const std::vector<std::string>& argv) {
    int code;
    try {
        code = process::run({ argv, s.dir, {} });
    } catch (const app::Failure& e) {
        throw app::Failure("building " + s.package + ": " + e.what());
    }
    if (code != 0)
        throw app::Failure("building " + s.package + " failed: `" + process::shown(argv) + "` exited " + std::to_string(code));
}

/* Removes a folder a build made, so the next build starts from nothing. */
static void remove_output(const config::BuildStep& s, const config::fs::path& p) {
    std::error_code ec;
    config::fs::remove_all(p, ec);
    if (ec) throw app::Failure("building " + s.package + ": cannot remove " + config::to_utf8(p) + ", " + ec.message());
}

int build_steps(app::Context& ctx, const config::Build& b, bool clean) {
    int built = 0;
    for (auto& s : b.steps) {
        if (s.configure.empty() && s.commands.empty()) continue;
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "building " + s.package));
        std::fflush(stdout);
        /* a build folder just removed configures again without a question */
        bool fresh = false;
        if (clean) {
            if (s.clean.empty()) ctx.out.note(s.package + " builds with its own commands, so rant removes nothing of it");
            for (auto& c : s.clean) remove_output(s, c);
            fresh = true;
        } else if (!s.moved_from.empty()) {
            if (!ui::confirm("The build folder of " + s.package + " was made in " + config::to_utf8(s.moved_from) +
                                 ", which CMake cannot use here. Remove it and configure again?",
                             ctx.yes))
                throw app::Failure(s.package + " was not built, `rant build " + s.package + " --clean` makes its build folder again");
            remove_output(s, s.build_dir);
            fresh = true;
        }
        if (!s.configure.empty() && (fresh || s.unconfigured)) {
            if (!fresh && !ui::confirm(s.package + " is not configured yet. Configure it with `" + process::shown(s.configure) + "`?", ctx.yes))
                throw app::Failure(s.package + " was not configured, so not built");
            run_command(s, s.configure);
        }
        for (auto& c : s.commands) run_command(s, c);
        built++;
    }
    return built;
}

bool build_for(app::Context& ctx, const std::vector<config::Plan>& plans) {
    if (ctx.args.has("no-build")) return true;
    const config::Workspace& ws = ctx.require_scope(true);
    std::vector<std::string> packages;
    for (auto& plan : plans)
        for (auto& i : plan.instances) {
            std::string pkg = i.type.substr(0, i.type.find('/'));
            bool known = std::any_of(ws.packages.begin(), ws.packages.end(), [&](const config::Package& p) { return p.name == pkg && !p.installed; });
            if (known && std::find(packages.begin(), packages.end(), pkg) == packages.end()) packages.push_back(pkg);
        }
    if (packages.empty()) return true;
    config::Build b = config::plan_build(ctx.cwd(), packages);
    try {
        require(ctx, b.diagnostics);
        if (build_steps(ctx, b, false) > 0) ctx.out.line();
        return true;
    } catch (const app::Failure& e) {
        if (*e.what()) ctx.out.error(std::string(e.what()) + ", starting the last build");
        else ctx.out.error("the build could not be planned, starting the last build");
        ctx.out.line();
        return false;
    }
}

app::OptionSpec no_build_option() { return { "no-build", 0, "", "start what is built already, without building first" }; }

static int run(app::Context& ctx) {
    const config::Workspace& ws = ctx.require_workspace(true);
    config::Build b = config::plan_build(ctx.cwd(), ctx.args.words);
    require(ctx, b.diagnostics);
    if (ctx.args.has("dry-run")) {
        dry_run(ctx, ws, b);
        return 0;
    }
    int built = build_steps(ctx, b, ctx.args.has("clean"));
    if (built == 0) ctx.out.line("nothing to build: no package here has a build step");
    else ctx.out.line("built " + std::to_string(built) + (built == 1 ? " package" : " packages"));
    /* only what the workspace itself names, never an installed package */
    std::vector<std::string> groups, nodes;
    auto installed = [&](const std::string& package) {
        return std::any_of(ws.packages.begin(), ws.packages.end(), [&](const config::Package& p) { return p.installed && p.name == package; });
    };
    for (auto& g : ws.groups)
        if (!installed(g.name.substr(0, g.name.find('/')))) groups.push_back(g.name);
    for (auto& p : ws.packages)
        for (auto& n : p.nodes)
            if (!p.installed) nodes.push_back(n.name);
    for (auto& n : ws.loose) nodes.push_back(n.name);
    note_names(ctx, groups, nodes);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    std::vector<std::string> out;
    for (auto& p : r.packages(false))
        if (std::find(r.words.begin(), r.words.end(), p) == r.words.end()) out.push_back(p);
    return { out };
}

app::Command build() {
    app::Command c{ "build", "[package...]", "build packages in dependency order", app::Section::Workspace,
                    { { "dry-run", 0, "", "show the packages, dependencies, order and commands without building" },
                      { "clean", 0, "", "remove each package's build output first, then configure and build from nothing" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
