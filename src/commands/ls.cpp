#include <algorithm>
#include <set>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/kinds.hpp"
#include "complete/complete.hpp"
#include "library/uses.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "process/command.hpp"
#include "run/nodes.hpp"
#include "ui/table.hpp"
#include "ui/terminal.hpp"
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
static std::string node_cell(app::Context& ctx, const Row& r) {
    if (!ctx.out.color()) return r.name;
    return ctx.out.paint(r.on_mesh ? ui::Style::Green : ui::Style::Amber, "\xe2\x97\x8f") + " " + r.name;
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

/* The width long lists spread across, 0 for one item a line when stdout is no terminal. */
static size_t width() { return ui::is_terminal(stdout) ? (size_t)ui::terminal_size().cols : 0; }

static void section(app::Context& ctx, const char* title, const std::vector<std::string>& lines) {
    ctx.out.line(ctx.out.paint(ui::Style::Bold, title));
    for (auto& l : lines) ctx.out.line(l);
    if (lines.empty()) ctx.out.line(ctx.out.paint(ui::Style::Faint, "  (none)"));
}

/* The running nodes and entities, the default. kind narrows to nodes or one entity kind. */
static int list_mesh(app::Context& ctx, std::optional<Kind> kind, const std::string& pattern) {
    bool show_nodes = !kind || *kind == Kind::Node, show_entities = !kind || *kind != Kind::Node;
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    Listing nodes;
    if (show_nodes) nodes = list_nodes(ctx, mesh, pattern);
    std::vector<rant::Entity> entities;
    if (show_entities)
        for (auto& e : mesh.entities())
            if (util::name_matches(pattern, e.name) && entity_matches(kind.value_or(Kind::Entity), mesh::kind_name(e.kind)))
                entities.push_back(e);

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

    if (show_nodes) {
        std::vector<std::string> lines;
        auto cells = [&](const std::vector<Row>& rows) {
            std::vector<std::vector<std::string>> out;
            for (auto& r : rows) out.push_back({ node_cell(ctx, r) });
            return out;
        };
        for (auto& g : nodes.groups) {
            lines.push_back("  " + ctx.out.paint(ui::Style::Faint, g.header + ":"));
            for (auto& l : ui::columns(cells(g.nodes), width(), "    ")) lines.push_back(l);
        }
        for (auto& l : ui::columns(cells(nodes.loose), width(), "  ")) lines.push_back(l);
        section(ctx, "NODES", lines);
    }
    if (show_nodes && show_entities) ctx.out.line();
    if (show_entities) {
        std::vector<std::vector<std::string>> cells;
        for (auto& e : entities) cells.push_back({ e.name, ctx.out.paint(kind_style(e.kind), mesh::kind_name(e.kind)) });
        section(ctx, "ENTITIES", ui::columns(cells, width(), "  "));
    }
    return 0;
}

/* The Rant version a package uses, the installed one for Python. */
static std::string rant_version(const config::Package& p) {
    std::string out;
    for (auto& u : library::in_folder(p.dir))
        if (u.version && (out.empty() || u.how != "pyproject")) out = *u.version;
    return out;
}

static std::string joined(const std::vector<std::string>& v, const char* sep = " ") {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : sep) + x;
    return s;
}

/* A group's params in short: `speed=1`, or `target=?` for a required one. */
static std::string params_short(const config::GroupInfo& g) {
    std::vector<std::string> v;
    for (auto& p : g.params) v.push_back(p.name + "=" + (p.default_value ? *p.default_value : "?"));
    return joined(v);
}

/* Packages, node types or groups: what the workspace offers, running or not. */
static int list_workspace(app::Context& ctx, Kind kind, const std::string& pattern) {
    const config::Workspace& ws = ctx.require_workspace(true);
    json out = json::array();
    ui::Table t;
    auto dim = [&](const std::string& s) { return ctx.out.paint(ui::Style::Dim, s); };
    const char* title = "";
    if (kind == Kind::Package) {
        title = "PACKAGES";
        for (auto& p : ws.packages) {
            if (!util::name_matches(pattern, p.name)) continue;
            std::string version = rant_version(p);
            out.push_back({ { "name", p.name }, { "kinds", p.kinds }, { "folder", config::to_utf8(p.dir) },
                            { "rant", version.empty() ? json(nullptr) : json(version) } });
            t.row({ p.name, dim(joined(p.kinds, ", ")), version.empty() ? dim("no Rant") : "Rant " + version, dim(ctx.shown(p.dir)) });
        }
    } else if (kind == Kind::Type) {
        title = "NODE TYPES";
        std::vector<const config::NodeType*> types;
        for (auto& p : ws.packages)
            for (auto& n : p.nodes) types.push_back(&n);
        for (auto& n : ws.loose) types.push_back(&n);
        for (auto* n : types) {
            if (!util::name_matches(pattern, n->ref())) continue;
            out.push_back({ { "name", n->ref() }, { "kind", config::kind_name(n->kind) },
                            { "path", n->path ? json(config::to_utf8(*n->path)) : json(nullptr) }, { "run", n->run } });
            t.row({ n->ref(), dim(config::kind_name(n->kind)), dim(n->path ? ctx.shown(*n->path) : process::shown(n->run)) });
        }
    } else {
        title = "GROUPS";
        for (auto& g : ws.groups) {
            if (!util::name_matches(pattern, g.name)) continue;
            config::GroupInfo info = config::describe_group(ctx.cwd, g.name);
            out.push_back({ { "name", g.name }, { "file", config::to_utf8(g.file) }, { "description", info.description },
                            { "params", params_short(info) } });
            t.row({ g.name, dim(params_short(info)), info.description });
        }
    }
    if (ctx.json) ctx.out.line(out.dump(2));
    else section(ctx, title, t.lines());
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    std::optional<Kind> kind;
    size_t at = 0;
    if (!w.empty() && (kind = kind_named(w[0]))) at = 1;
    if (w.size() > at + 1) throw app::UsageError("ls takes a kind and one pattern at most, such as `rant ls types cam`");
    std::string pattern = at < w.size() ? w[at] : "";
    if (kind && !on_mesh(*kind)) return list_workspace(ctx, *kind, pattern);
    return list_mesh(ctx, kind, pattern);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { kind_words(true) };
    return {};
}

app::Command ls() {
    app::Command c{ "ls", "[kind] [pattern]",
                    "list the running nodes and entities, or packages, types or groups",
                    app::Section::Mesh, {}, run };
    c.complete = complete_words;
    return c;
}

}
