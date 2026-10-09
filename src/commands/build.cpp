#include <algorithm>
#include <cstdio>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
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
        packages.push_back({ { "name", p.name }, { "dir", config::to_utf8(p.dir) }, { "nodes", node_types_json(p.nodes) } });
    json edges = json::array();
    for (auto& e : b.edges) edges.push_back({ { "from", e.from }, { "to", e.to }, { "source", e.source } });
    json steps = json::array();
    for (auto& s : b.steps) {
        json step = { { "package", s.package }, { "dir", config::to_utf8(s.dir) } };
        step["configure"] = s.configure.empty() ? json(nullptr) : json(s.configure);
        step["commands"] = s.commands;
        steps.push_back(step);
    }
    return { { "workspace", config::to_utf8(ws.root) },
             { "packages", packages },
             { "loose", node_types_json(ws.loose) },
             { "dependencies", edges },
             { "build", steps } };
}

/* Everything a build would do, and why: the packages, their node types, every dependency
 * with where it was found, the order and the exact commands. */
static void dry_run(app::Context& ctx, const config::Workspace& ws, const config::Build& b) {
    if (ctx.json) {
        ctx.out.line(dry_run_json(ws, b).dump(2));
        return;
    }
    std::vector<const config::Package*> sorted;
    for (auto& p : ws.packages) sorted.push_back(&p);
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
    int n = 0;
    for (auto& s : b.steps) {
        ctx.out.line("  " + std::to_string(++n) + ". " + s.package);
        if (!s.configure.empty())
            ctx.out.line("     " + process::shown(s.configure) + ctx.out.paint(ui::Style::Dim, "  (asks first)"));
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

static int run(app::Context& ctx) {
    const config::Workspace& ws = ctx.require_workspace(true);
    config::Build b = config::plan_build(ctx.cwd(), ctx.args.words);
    require(ctx, b.diagnostics);
    if (ctx.args.has("dry-run")) {
        dry_run(ctx, ws, b);
        return 0;
    }
    int built = 0;
    for (auto& s : b.steps) {
        if (s.configure.empty() && s.commands.empty()) continue;
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "building " + s.package));
        std::fflush(stdout);
        if (!s.configure.empty()) {
            if (!ui::confirm(s.package + " has no build directory yet. Configure it with `" + process::shown(s.configure) + "`?", ctx.yes))
                throw app::Failure("not configured, so " + s.package + " was not built");
            run_command(s, s.configure);
        }
        for (auto& c : s.commands) run_command(s, c);
        built++;
    }
    if (built == 0) ctx.out.line("nothing to build: no package here has a build step");
    else ctx.out.line("built " + std::to_string(built) + (built == 1 ? " package" : " packages"));
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    std::vector<std::string> out;
    for (auto& p : r.packages())
        if (std::find(r.words.begin(), r.words.end(), p) == r.words.end()) out.push_back(p);
    return { out };
}

app::Command build() {
    app::Command c{ "build", "[package...]", "build packages in dependency order", app::Section::Workspace,
                    { { "dry-run", 0, "", "show the packages, dependencies, order and commands without building" } }, run };
    c.complete = complete_words;
    return c;
}

}
