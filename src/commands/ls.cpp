#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "util/match.hpp"

namespace commands {

using mesh::json;

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    bool show_nodes = true, show_entities = true;
    size_t at = 0;
    if (!w.empty() && (w[0] == "nodes" || w[0] == "entities")) {
        show_nodes = w[0] == "nodes";
        show_entities = !show_nodes;
        at = 1;
    }
    if (w.size() > at + 1) throw app::UsageError("ls takes one pattern at most");
    std::string pattern = at < w.size() ? w[at] : "";

    mesh::Client mesh(ctx.domain);
    mesh.settle();

    std::vector<std::string> nodes;
    for (auto& p : mesh.peers())
        if (util::name_matches(pattern, p.name)) nodes.push_back(p.name);
    std::vector<rant::Entity> entities;
    for (auto& e : mesh.entities())
        if (util::name_matches(pattern, e.name)) entities.push_back(e);

    if (ctx.json) {
        json out = json::object();
        if (show_nodes) {
            out["nodes"] = json::array();
            for (auto& n : nodes) out["nodes"].push_back({ { "name", n } });
        }
        if (show_entities) {
            out["entities"] = json::array();
            for (auto& e : entities) out["entities"].push_back({ { "name", e.name }, { "kind", mesh::kind_name(e.kind) } });
        }
        ctx.out.line(out.dump(2));
        return 0;
    }

    auto none = ctx.out.paint(ui::Style::Dim, "  (none)");
    if (show_nodes) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "NODES"));
        for (auto& n : nodes) ctx.out.line("  " + n);
        if (nodes.empty()) ctx.out.line(none);
    }
    if (show_nodes && show_entities) ctx.out.line();
    if (show_entities) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "ENTITIES"));
        size_t width = 0;
        for (auto& e : entities) width = std::max(width, e.name.size());
        for (auto& e : entities) {
            std::string name = e.name;
            name.resize(width + 3, ' ');
            ctx.out.line("  " + name + ctx.out.paint(ui::Style::Dim, mesh::kind_name(e.kind)));
        }
        if (entities.empty()) ctx.out.line(none);
    }
    return 0;
}

app::Command ls() {
    return { "ls", "[nodes|entities] [pattern]", "list the running nodes and what they talk through",
             app::Section::Mesh, {}, run };
}

}
