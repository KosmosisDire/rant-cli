#include <algorithm>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "ui/table.hpp"

namespace commands {

using json = nlohmann::ordered_json;

static std::string shown_path(const config::Workspace& ws, const config::fs::path& p) {
    std::error_code ec;
    auto rel = config::fs::relative(p, ws.root, ec);
    return config::to_utf8(ec || rel.empty() ? p : rel);
}

static std::string joined(const std::vector<std::string>& v) {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : " ") + x;
    return s;
}

/* What discovery found, in full: the packages and the node types each holds. */
static int dry_run(app::Context& ctx, const config::Workspace& ws) {
    if (ctx.json) {
        json out = { { "workspace", config::to_utf8(ws.root) }, { "packages", json::array() } };
        for (auto& p : ws.packages) {
            json nodes = json::array();
            for (auto& n : p.nodes)
                nodes.push_back({ { "name", n.name },
                                  { "kind", config::kind_name(n.kind) },
                                  { "path", n.path ? json(config::to_utf8(*n.path)) : json(nullptr) },
                                  { "run", n.run },
                                  { "cwd", config::to_utf8(n.cwd) } });
            out["packages"].push_back({ { "name", p.name }, { "dir", config::to_utf8(p.dir) }, { "nodes", nodes } });
        }
        ctx.out.line(out.dump(2));
        return 0;
    }

    std::vector<const config::Package*> sorted;
    for (auto& p : ws.packages) sorted.push_back(&p);
    std::sort(sorted.begin(), sorted.end(), [](auto* a, auto* b) { return a->name < b->name; });
    ui::Table packages, nodes;
    for (auto* pp : sorted) {
        const config::Package& p = *pp;
        packages.row({ p.name, ctx.out.paint(ui::Style::Dim, shown_path(ws, p.dir)) });
        for (auto& n : p.nodes)
            nodes.row({ n.ref(), ctx.out.paint(ui::Style::Dim, config::kind_name(n.kind)),
                        n.path ? shown_path(ws, *n.path) : joined(n.run) });
    }
    auto none = ctx.out.paint(ui::Style::Dim, "  (none)");
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "PACKAGES"));
    for (auto& l : packages.lines()) ctx.out.line(l);
    if (packages.empty()) ctx.out.line(none);
    ctx.out.line();
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "NODE TYPES"));
    for (auto& l : nodes.lines()) ctx.out.line(l);
    if (nodes.empty()) ctx.out.line(none);
    return 0;
}

static int run(app::Context& ctx) {
    const config::Workspace& ws = ctx.require_workspace(true);
    if (ctx.args.has("dry-run")) return dry_run(ctx, ws);
    throw app::Failure("building is not done yet, `rant build --dry-run` lists what would be built");
}

app::Command build() {
    return { "build", "[package...]", "build packages in dependency order", app::Section::Workspace,
             { { "dry-run", 0, "", "show the packages, node types and commands without building" } }, run };
}

}
