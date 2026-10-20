#include <algorithm>
#include <cstdlib>
#include <map>
#include <optional>
#include <set>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/kinds.hpp"
#include "complete/complete.hpp"
#include "library/uses.hpp"
#include "mesh/client.hpp"
#include "mesh/schema_text.hpp"
#include "mesh/values.hpp"
#include "packages/packages.hpp"
#include "process/command.hpp"
#include "run/nodes.hpp"
#include "ui/table.hpp"
#include "ui/terminal.hpp"
#include "util/match.hpp"

namespace commands {

using mesh::json;

/* One node as ls lists it. A node rant started that is not on the mesh is still joining,
 * or never joins since it does not use Rant. Unknown with --no-mesh, or for a program
 * declared to never join. */
struct Row {
    std::string             name;
    std::optional<bool>     on_mesh;
    std::string             type;      /* the node type of one rant started, else empty */
    std::optional<uint16_t> domain;    /* the domain of one rant started */
    std::string             group;     /* the group whose own block made one rant started */
};

/* The domain a started node joined: the RANT_DOMAIN rant gave it, else Rant's 0. */
static uint16_t started_domain(const state::Instance& i) {
    auto it = i.env.find("RANT_DOMAIN");
    return it == i.env.end() ? 0 : static_cast<uint16_t>(std::strtoul(it->second.c_str(), nullptr, 10));
}

/* The running nodes as ls shows them: each group root with its nodes, then every node
 * outside a group, started by rant or not. A node two roots share shows under both. */
struct Listing {
    struct Group {
        std::string      header;    /* "nav" or "nav target=red_bin" */
        std::vector<Row> nodes;
        bool             inside = false;    /* a group the one above includes */
    };
    std::vector<Group> groups;
    std::vector<Row>   loose;
};

/* Without a mesh only the nodes this workspace started, with no word on whether they joined. */
static Listing list_nodes(app::Context& ctx, mesh::Client* mesh, const std::string& pattern) {
    state::State st = run::snapshot(ctx);
    std::map<uint32_t, std::string> managed;
    if (mesh) managed = run::managed_peers(*mesh, st);
    std::set<std::string> on_mesh;
    for (auto& [_, name] : managed) on_mesh.insert(name);
    auto row = [&](const state::Instance& i) {
        std::optional<bool> joined = mesh && i.expect_mesh ? std::optional<bool>(on_mesh.count(i.name) > 0) : std::nullopt;
        return Row{ i.name, joined, i.type, started_domain(i), i.group };
    };

    Listing out;
    for (auto& r : st.roots) {
        if (r.kind != "group") continue;
        /* the params show only when the group runs more than once */
        size_t runs = std::count_if(st.roots.begin(), st.roots.end(), [&](const state::Root& o) { return o.kind == "group" && o.name == r.name; });
        Listing::Group g{ runs > 1 ? r.key().substr(6) : r.name, {} };
        /* the nodes of groups it includes go under those groups, in the order they first come */
        std::vector<Listing::Group> inside;
        for (auto& i : st.instances) {
            if (!std::count(i.roots.begin(), i.roots.end(), r.key()) || !util::name_matches(pattern, i.name)) continue;
            if (i.group.empty() || i.group == r.name) {
                g.nodes.push_back(row(i));
                continue;
            }
            auto it = std::find_if(inside.begin(), inside.end(), [&](const Listing::Group& x) { return x.header == i.group; });
            if (it == inside.end()) it = inside.insert(inside.end(), { i.group, {}, true });
            it->nodes.push_back(row(i));
        }
        if (g.nodes.empty() && inside.empty()) continue;
        out.groups.push_back(g);
        out.groups.insert(out.groups.end(), inside.begin(), inside.end());
    }
    for (auto& i : st.instances) {
        bool in_group = std::any_of(i.roots.begin(), i.roots.end(), [](const std::string& k) { return k.rfind("group ", 0) == 0; });
        if (!in_group && util::name_matches(pattern, i.name)) out.loose.push_back(row(i));
    }
    if (mesh)
        for (auto& p : mesh->peers())
            if (!managed.count(p.id) && util::name_matches(pattern, p.name)) out.loose.push_back({ p.name, true });
    return out;
}

/* The explorer's colors: a node's dot green on the mesh and amber while joining, and each
 * entity kind in the color the explorer's mesh view draws it. */
static std::string node_cell(app::Context& ctx, const Row& r) {
    if (!ctx.out.color() || !r.on_mesh) return r.name;
    return ctx.out.paint(*r.on_mesh ? ui::Style::Green : ui::Style::Amber, "\xe2\x97\x8f") + " " + r.name;
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

/* The nodes this scope could start that are not running. */
static std::vector<const config::NodeType*> not_running(app::Context& ctx, const std::string& pattern) {
    std::vector<const config::NodeType*> out;
    if (!ctx.scope()) return out;
    const config::Workspace& ws = ctx.require_scope(true);
    state::State st = run::snapshot(ctx);
    auto add = [&](const config::NodeType& n) {
        bool runs = std::any_of(st.instances.begin(), st.instances.end(), [&](const state::Instance& i) { return i.type == n.planned(); });
        if (!runs && util::name_matches(pattern, n.ref())) out.push_back(&n);
    };
    for (auto& p : ws.packages)
        for (auto& n : p.nodes) add(n);
    for (auto& n : ws.loose) add(n);
    return out;
}

/* The named types the mesh's entities use, each with the entities that use it. */
static int list_types(app::Context& ctx, const std::string& pattern) {
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    json out = json::array();
    ui::Table t;
    for (auto& [name, use] : mesh::types_in_use(mesh.entities())) {
        if (!util::name_matches(pattern, name)) continue;
        out.push_back({ { "type", name }, { "used_by", use.entities } });
        std::string users;
        for (auto& e : use.entities) users += (users.empty() ? "" : ", ") + e;
        t.row({ name, ctx.out.paint(ui::Style::Dim, users) });
    }
    if (ctx.json) ctx.out.line(out.dump(2));
    else section(ctx, "TYPES", t.lines());
    return 0;
}

/* The nodes, running ones and those that could start, or the entities of one kind. */
static int list_mesh(app::Context& ctx, Kind kind, const std::string& pattern) {
    if (kind == Kind::Type) return list_types(ctx, pattern);
    bool show_nodes = kind == Kind::Node;
    bool no_mesh = ctx.args.has("no-mesh");
    if (no_mesh && !show_nodes) throw app::UsageError("--no-mesh goes with `rant ls nodes`");
    std::optional<mesh::Client> mesh;
    if (!no_mesh) {
        mesh.emplace(ctx.domain());
        mesh->settle();
    }
    Listing nodes;
    std::vector<const config::NodeType*> idle;
    std::vector<rant::Entity> entities;
    if (show_nodes) {
        nodes = list_nodes(ctx, mesh ? &*mesh : nullptr, pattern);
        idle = not_running(ctx, pattern);
    } else {
        for (auto& e : mesh->entities())
            if (util::name_matches(pattern, e.name) && entity_matches(kind, mesh::kind_name(e.kind))) entities.push_back(e);
    }

    if (ctx.json) {
        json out = json::object();
        auto rows = [](const std::vector<Row>& v) {
            json a = json::array();
            for (auto& r : v) {
                json row = { { "name", r.name } };
                if (!r.type.empty()) row["type"] = r.type;
                if (r.domain) row["domain"] = *r.domain;
                if (!r.group.empty()) row["group"] = r.group;
                if (r.on_mesh) row["on_mesh"] = *r.on_mesh;
                a.push_back(row);
            }
            return a;
        };
        if (show_nodes) {
            /* the mesh this listing reads, the workspace's, so a tool can join the same one */
            out["domain"] = ctx.domain();
            out["groups"] = json::array();
            /* a group a run includes is part of that run, its nodes there with their own `group` */
            for (size_t i = 0; i < nodes.groups.size(); i++) {
                if (nodes.groups[i].inside) continue;
                json n = rows(nodes.groups[i].nodes);
                for (size_t k = i + 1; k < nodes.groups.size() && nodes.groups[k].inside; k++)
                    for (auto& r : rows(nodes.groups[k].nodes)) n.push_back(r);
                out["groups"].push_back({ { "group", nodes.groups[i].header }, { "nodes", n } });
            }
            out["nodes"] = rows(nodes.loose);
            out["not_running"] = json::array();
            for (auto* n : idle)
                out["not_running"].push_back({ { "name", n->ref() }, { "kind", config::kind_name(n->kind) },
                                               { "path", n->path ? json(config::to_utf8(*n->path)) : json(nullptr) } });
        } else {
            out["entities"] = json::array();
            for (auto& e : entities) out["entities"].push_back({ { "name", e.name }, { "kind", mesh::kind_name(e.kind) } });
        }
        ctx.out.line(out.dump(2));
        return 0;
    }

    if (show_nodes) {
        /* every list of nodes on one grid, those outside a group under a header of their own
           when there are groups, so the columns line up all the way down */
        std::vector<std::string> headers;
        std::vector<std::vector<std::vector<std::string>>> lists;
        auto cells = [&](const std::vector<Row>& rows) {
            std::vector<std::vector<std::string>> out;
            for (auto& r : rows) out.push_back({ node_cell(ctx, r) });
            return out;
        };
        for (auto& g : nodes.groups) {
            headers.push_back(g.header);
            lists.push_back(cells(g.nodes));
        }
        bool grouped = !nodes.groups.empty();
        if (!nodes.loose.empty()) {
            headers.push_back("other");
            lists.push_back(cells(nodes.loose));
        }
        std::vector<std::string> lines;
        auto laid = ui::columns(lists, width(), grouped ? "    " : "  ");
        for (size_t i = 0; i < laid.size(); i++) {
            /* a group a run includes sits one step in, under that run */
            std::string in = i < nodes.groups.size() && nodes.groups[i].inside ? "  " : "";
            if (grouped) lines.push_back("  " + in + ctx.out.paint(ui::Style::Faint, headers[i] + ":"));
            for (auto& l : laid[i]) lines.push_back(in + l);
        }
        section(ctx, "NODES", lines);
        ui::Table t;
        auto dim = [&](const std::string& x) { return ctx.out.paint(ui::Style::Dim, x); };
        for (auto* n : idle) t.row({ n->ref(), dim(config::kind_name(n->kind)), dim(n->path ? ctx.shown(*n->path) : process::shown(n->run)) });
        ctx.out.line();
        section(ctx, "NOT RUNNING", t.lines());
    } else {
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

/* Packages or groups: what the workspace and the installed packages offer, running or not. */
static int list_workspace(app::Context& ctx, Kind kind, const std::string& pattern) {
    const config::Workspace& ws = ctx.require_scope(true);
    json out = json::array();
    ui::Table t;
    auto dim = [&](const std::string& s) { return ctx.out.paint(ui::Style::Dim, s); };
    const char* title = "";
    if (kind == Kind::Package) {
        title = "PACKAGES";
        for (auto& p : ws.packages) {
            if (!util::name_matches(pattern, p.name)) continue;
            std::string version = rant_version(p);
            json files = json::array();
            for (auto& f : p.files) files.push_back(config::to_utf8(f));
            std::optional<packages::Record> rec = p.installed ? packages::record(p.name) : std::nullopt;
            bool linked = p.installed && packages::is_link(packages::dir() / p.name);
            json item = { { "name", p.name }, { "kinds", p.kinds }, { "folder", config::to_utf8(p.dir) }, { "files", files },
                          { "rant", version.empty() ? json(nullptr) : json(version) }, { "installed", p.installed } };
            if (rec && !rec->version.empty()) item["version"] = rec->version;
            if (rec) item["source"] = rec->repo.empty() ? config::to_utf8(rec->path) : rec->repo;
            out.push_back(item);
            std::string state = linked ? "linked" : rec && !rec->version.empty() ? "installed " + rec->version : "installed";
            std::string uses = p.installed ? dim(state) : version.empty() ? dim("no Rant") : "Rant " + version;
            t.row({ p.name, dim(joined(p.kinds, ", ")), uses, dim(ctx.shown(p.dir)) });
        }
    } else {
        title = "GROUPS";
        for (auto& g : ws.groups) {
            if (!util::name_matches(pattern, g.name)) continue;
            config::GroupInfo info = config::describe_group(ctx.cwd(), g.name);
            json item = { { "name", g.name }, { "file", config::to_utf8(g.file) }, { "description", info.description },
                          { "params", params_short(info) }, { "includes", info.includes } };
            if (ctx.json) {
                /* its own nodes as the defaults name them, none when a param has no default */
                json nodes = json::array();
                for (auto& i : config::plan_group(ctx.cwd(), g.name, {}).instances)
                    if (i.group == g.name) nodes.push_back({ { "name", i.name }, { "type", i.type } });
                item["nodes"] = nodes;
            }
            out.push_back(item);
            t.row({ g.name, dim(params_short(info)), info.description });
        }
    }
    if (ctx.json) ctx.out.line(out.dump(2));
    else section(ctx, title, t.lines());
    return 0;
}

/* What rant install can install, each from the first source that names it, with what its
 * package.rant says at the repo's default branch. */
static int list_available(app::Context& ctx, const std::string& pattern) {
    packages::Sources sources;
    auto dim = [&](const std::string& s) { return ctx.out.paint(ui::Style::Dim, s); };
    json out = json::array();
    ui::Table t;
    for (auto& s : sources.all()) {
        if (!util::name_matches(pattern, s.name)) continue;
        std::string from = s.repo.empty() ? config::to_utf8(s.path) : s.repo, description;
        try {
            description = packages::about(s, false).description;
        } catch (const app::Failure& e) {
            ctx.out.warn(s.name + ": " + e.what());
        }
        std::optional<packages::Record> rec = packages::record(s.name);
        std::string installed = !rec ? "" : rec->version.empty() ? "installed" : "installed " + rec->version;
        out.push_back({ { "name", s.name }, { s.repo.empty() ? "path" : "repo", from }, { "description", description },
                        { "installed", rec ? json(rec->version.empty() ? json(true) : json(rec->version)) : json(false) } });
        t.row({ s.name, dim(from), dim(installed), description });
    }
    if (ctx.json) ctx.out.line(out.dump(2));
    else section(ctx, "AVAILABLE", t.lines());
    return 0;
}

/* Where rant install looks, in order: the packages sources.rant names, the official database,
 * then the databases sources.rant adds. */
static int list_sources(app::Context& ctx) {
    packages::Sources sources;
    auto dim = [&](const std::string& s) { return ctx.out.paint(ui::Style::Dim, s); };
    json out = json::array();
    ui::Table t;
    for (auto& l : sources.local().packages) {
        std::string from = l.repo.empty() ? config::to_utf8(l.path) : l.repo;
        out.push_back({ { "package", l.name }, { l.repo.empty() ? "path" : "repo", from } });
        t.row({ l.name, dim("package"), from });
    }
    for (auto& d : sources.databases()) {
        bool official = d == packages::official;
        out.push_back({ { "database", d }, { "official", official } });
        t.row({ d, dim("database"), official ? dim("official") : "" });
    }
    if (ctx.json) {
        ctx.out.line(out.dump(2));
        return 0;
    }
    section(ctx, "SOURCES", t.lines());
    if (sources.local().packages.empty() && sources.local().databases.empty())
        ctx.out.note("name your own in " + config::to_utf8(packages::sources_file()));
    return 0;
}

/* With nothing to list, the help, which names every kind. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty() || ctx.args.has("help")) {
        std::string s = app::command_help(ls()) + "\nKinds:\n";
        for (auto& [word, listed] : kinds_listed()) {
            word.resize(std::max<size_t>(word.size(), 22), ' ');
            s += "  " + word + " " + listed + "\n";
        }
        ctx.out.line(s);
        return 0;
    }
    std::optional<Kind> kind = kind_named(w[0]);
    if (!kind) throw app::UsageError("`" + w[0] + "` is not a kind ls lists, see `rant ls`");
    if (w.size() > 2) throw app::UsageError("ls takes a kind and one pattern at most, such as `rant ls types cam`");
    std::string pattern = w.size() == 2 ? w[1] : "";
    if (ctx.args.has("available") && *kind != Kind::Package) throw app::UsageError("--available goes with `rant ls packages`");
    if (*kind == Kind::Source) {
        if (!pattern.empty()) throw app::UsageError("ls sources takes no pattern");
        return list_sources(ctx);
    }
    if (*kind == Kind::Package && ctx.args.has("available")) return list_available(ctx, pattern);
    if (!on_mesh(*kind)) return list_workspace(ctx, *kind, pattern);
    return list_mesh(ctx, *kind, pattern);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { kind_words(true) };
    return {};
}

app::Command ls() {
    app::Command c{ "ls", "<kind> [pattern]",
                    "list nodes, entities, types, packages or groups",
                    app::Section::Mesh,
                    { { "no-mesh", 0, "", "with nodes: only what this workspace declares and started, without joining the mesh" },
                      { "available", 0, "", "with packages: what `rant install` can install, from sources.rant and the package databases" } },
                    run };
    c.own_help = true;
    c.complete = complete_words;
    return c;
}

}
