#include <algorithm>
#include <set>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "run/nodes.hpp"
#include "util/match.hpp"

namespace commands {

using mesh::json;

/* One node as ls lists it. A node rant started that is not on the mesh is still joining,
 * or never joins since it does not use Rant. */
struct Row {
    std::string name;
    bool        on_mesh = true;
};

/* The running nodes as ls shows them: each group root with its nodes, then every node
 * outside a group, started by rant or not. A node two roots share shows under both. */
struct Listing {
    struct Group {
        std::string      header;    /* "nav" or "nav target=red_bin" */
        std::vector<Row> nodes;
    };
    std::vector<Group> groups;
    std::vector<Row>   loose;
};

static Listing list_nodes(app::Context& ctx, mesh::Client& mesh, const std::string& pattern) {
    state::State st = run::snapshot(ctx);
    auto managed = run::managed_peers(mesh, st);
    std::set<std::string> on_mesh;
    for (auto& [_, name] : managed) on_mesh.insert(name);
    auto row = [&](const state::Instance& i) { return Row{ i.name, on_mesh.count(i.name) > 0 }; };

    Listing out;
    for (auto& r : st.roots) {
        if (r.kind != "group") continue;
        /* the params show only when the group runs more than once */
        size_t runs = std::count_if(st.roots.begin(), st.roots.end(), [&](const state::Root& o) { return o.kind == "group" && o.name == r.name; });
        Listing::Group g{ runs > 1 ? r.key().substr(6) : r.name, {} };
        for (auto& i : st.instances)
            if (std::count(i.roots.begin(), i.roots.end(), r.key()) && util::name_matches(pattern, i.name))
                g.nodes.push_back(row(i));
        if (!g.nodes.empty()) out.groups.push_back(g);
    }
    for (auto& i : st.instances) {
        bool in_group = std::any_of(i.roots.begin(), i.roots.end(), [](const std::string& k) { return k.rfind("group ", 0) == 0; });
        if (!in_group && util::name_matches(pattern, i.name)) out.loose.push_back(row(i));
    }
    for (auto& p : mesh.peers())
        if (!managed.count(p.id) && util::name_matches(pattern, p.name)) out.loose.push_back({ p.name, true });
    return out;
}

/* The explorer's colors: a node's dot green on the mesh and amber while joining, and each
 * entity kind in the color the explorer's mesh view draws it. */
static std::string node_line(app::Context& ctx, const Row& r, const std::string& indent) {
    if (!ctx.out.color()) return indent + r.name;
    return indent + ctx.out.paint(r.on_mesh ? ui::Style::Green : ui::Style::Amber, "\xe2\x97\x8f") + " " + r.name;
}

static ui::Style kind_style(rant::EntityKind k) {
    switch (k) {
    case rant::EntityKind::Function: return ui::Style::Accent;
    case rant::EntityKind::Variable: return ui::Style::Amber;
    case rant::EntityKind::Task:     return ui::Style::Green;
    case rant::EntityKind::Topic:    return ui::Style::Dim;
    }
    return ui::Style::Dim;
}

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
    Listing nodes;
    if (show_nodes) nodes = list_nodes(ctx, mesh, pattern);
    std::vector<rant::Entity> entities;
    if (show_entities)
        for (auto& e : mesh.entities())
            if (util::name_matches(pattern, e.name)) entities.push_back(e);

    if (ctx.json) {
        json out = json::object();
        auto rows = [](const std::vector<Row>& v) {
            json a = json::array();
            for (auto& r : v) a.push_back({ { "name", r.name }, { "on_mesh", r.on_mesh } });
            return a;
        };
        if (show_nodes) {
            out["groups"] = json::array();
            for (auto& g : nodes.groups) out["groups"].push_back({ { "group", g.header }, { "nodes", rows(g.nodes) } });
            out["nodes"] = rows(nodes.loose);
        }
        if (show_entities) {
            out["entities"] = json::array();
            for (auto& e : entities) out["entities"].push_back({ { "name", e.name }, { "kind", mesh::kind_name(e.kind) } });
        }
        ctx.out.line(out.dump(2));
        return 0;
    }

    auto none = ctx.out.paint(ui::Style::Faint, "  (none)");
    if (show_nodes) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "NODES"));
        for (auto& g : nodes.groups) {
            ctx.out.line("  " + ctx.out.paint(ui::Style::Faint, g.header + ":"));
            for (auto& r : g.nodes) ctx.out.line(node_line(ctx, r, "    "));
        }
        for (auto& r : nodes.loose) ctx.out.line(node_line(ctx, r, "  "));
        if (nodes.groups.empty() && nodes.loose.empty()) ctx.out.line(none);
    }
    if (show_nodes && show_entities) ctx.out.line();
    if (show_entities) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, "ENTITIES"));
        size_t width = 0;
        for (auto& e : entities) width = std::max(width, e.name.size());
        for (auto& e : entities) {
            std::string name = e.name;
            name.resize(width + 3, ' ');
            ctx.out.line("  " + name + ctx.out.paint(kind_style(e.kind), mesh::kind_name(e.kind)));
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
