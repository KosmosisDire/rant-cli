#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/kinds.hpp"
#include "complete/complete.hpp"
#include "config/lib.hpp"
#include "mesh/access.hpp"
#include "mesh/schema_text.hpp"
#include "process/command.hpp"
#include "run/nodes.hpp"
#include "ui/yaml.hpp"

namespace commands {

namespace fs = std::filesystem;
using mesh::json;

/* What a node does with an entity, in the words a reader expects for its kind. */
static std::vector<std::string> roles(const rant::Entity& e) {
    std::vector<std::string> out;
    bool topic = e.kind == rant::EntityKind::Topic, var = e.kind == rant::EntityKind::Variable;
    if (e.provides) out.push_back(topic ? "publishes" : var ? "owns" : "serves");
    if (e.consumes) out.push_back(topic ? "subscribes" : var ? "reads" : "calls");
    return out;
}

/* One answer of info: its fields in order, and the schemas among them, which print as
 * schema text rather than as a value. */
struct Answer {
    json                                     fields = json::object();
    std::vector<std::pair<std::string, rant::Schema>> schemas;
};

/* A node rant started, a node on the mesh, or both when the mesh node is the instance. */
struct NodeView {
    const state::Instance* instance = nullptr;
    const mesh::Peer*      peer = nullptr;
};

static Answer node_answer(mesh::Client& mesh, const NodeView& v) {
    Answer a;
    json& j = a.fields;
    j["node"] = v.instance ? v.instance->name : v.peer->name;
    if (v.instance) {
        j["type"] = v.instance->type;
        j["run"] = v.instance->argv;
        j["log"] = v.instance->log;
        j["roots"] = v.instance->roots;
    }
    if (!v.peer) {
        j["pid"] = v.instance->tracking.pid;
        j["mesh"] = "not on the mesh";
        return a;
    }
    j["address"] = v.peer->address;
    auto pids = mesh.pids({ v.peer->id });
    if (pids.count(v.peer->id)) j["pid"] = pids[v.peer->id];
    for (auto& e : mesh.entities_of(v.peer->id))
        for (auto& r : roles(e)) {
            if (!j.contains(r)) j[r] = json::array();
            j[r].push_back(e.name);
        }
    return a;
}

static Answer entity_answer(mesh::Client& mesh, const rant::Entity& e) {
    Answer a;
    json& j = a.fields;
    j[mesh::kind_name(e.kind)] = e.name;
    if (e.kind == rant::EntityKind::Function || e.kind == rant::EntityKind::Task) {
        a.schemas.emplace_back("request", e.schema);
        a.schemas.emplace_back("response", e.rsp_schema);
        if (e.kind == rant::EntityKind::Task) a.schemas.emplace_back("progress", e.progress_schema);
    } else {
        a.schemas.emplace_back("type", e.schema);
    }
    for (auto& [k, s] : a.schemas) j[k] = mesh::schema_text(s, nullptr);

    json providers = json::array(), consumers = json::array();
    for (auto& p : mesh.peers())
        for (auto& pe : mesh.entities_of(p.id))
            if (pe.kind == e.kind && pe.name == e.name) {
                if (pe.provides) providers.push_back(p.name);
                if (pe.consumes) consumers.push_back(p.name);
            }
    bool topic = e.kind == rant::EntityKind::Topic, var = e.kind == rant::EntityKind::Variable;
    j[topic ? "publishers" : var ? "owner" : "providers"] = providers;
    j[topic ? "subscribers" : var ? "readers" : "callers"] = consumers;
    if (var) {
        auto value = mesh::read_variable(mesh, e.name);
        j["value"] = value ? *value : json(nullptr);
    }
    return a;
}

/* An answer as YAML, keys lined up. A schema of one line sits after its key, a longer one
 * goes below it as a block. */
static void print_answer(app::Context& ctx, const Answer& a) {
    ui::Yaml yaml(&ctx.out, false);
    size_t width = 0;
    for (auto& [k, _] : a.fields.items()) width = std::max(width, k.size());
    for (auto& [k, v] : a.fields.items()) {
        auto schema = std::find_if(a.schemas.begin(), a.schemas.end(), [&](auto& s) { return s.first == k; });
        if (schema == a.schemas.end()) {
            ctx.out.line(yaml.entry(k, v, width));
            continue;
        }
        std::string text = mesh::schema_text(schema->second, &ctx.out);
        if (text.find('\n') == std::string::npos) {
            ctx.out.line(yaml.key(k, width) + " " + text);
            continue;
        }
        ctx.out.line(yaml.key(k, width) + " " + ctx.out.paint(ui::Style::Faint, "|"));
        size_t start = 0;
        while (start <= text.size()) {
            size_t end = text.find('\n', start);
            if (end == std::string::npos) end = text.size();
            std::string line = text.substr(start, end - start);
            ctx.out.line(line.empty() ? "" : "  " + line);
            start = end + 1;
        }
    }
}

/* Every node by that name: rant's instances first, each with its mesh node when it is on
 * the mesh, then mesh nodes rant did not start. */
static std::vector<NodeView> find_nodes(mesh::Client& mesh, const state::State& st,
                                        const std::vector<mesh::Peer>& peers, const std::string& name) {
    auto managed = run::managed_peers(mesh, st);
    std::vector<NodeView> out;
    auto instance_of = [&](uint32_t peer) -> const state::Instance* {
        auto m = managed.find(peer);
        if (m == managed.end()) return nullptr;
        for (auto& i : st.instances)
            if (i.name == m->second) return &i;
        return nullptr;
    };
    for (auto& inst : st.instances) {
        if (inst.name != name) continue;
        NodeView v{ &inst, nullptr };
        for (auto& p : peers)
            if (instance_of(p.id) == &inst) v.peer = &p;
        out.push_back(v);
    }
    for (auto& p : peers) {
        const state::Instance* inst = instance_of(p.id);
        if (p.name != name || (inst && inst->name == name)) continue;    /* shown with its instance */
        out.push_back({ inst, &p });
    }
    return out;
}

/* Every node type of the workspace, in packages and outside them. */
static std::vector<const config::NodeType*> all_types(const config::Workspace& ws) {
    std::vector<const config::NodeType*> out;
    for (auto& p : ws.packages)
        for (auto& n : p.nodes) out.push_back(&n);
    for (auto& n : ws.loose) out.push_back(&n);
    return out;
}

static std::string type_of(const config::NodeType& n) {
    /* a type outside every package is known by its file, as a plan names it */
    return n.package.empty() && n.path ? config::to_utf8(*n.path) : n.ref();
}

static Answer package_answer(app::Context& ctx, const config::Package& p) {
    Answer a;
    json& j = a.fields;
    j["package"] = p.name;
    j["folder"] = ctx.shown(p.dir);
    j["kind"] = p.kinds;
    json rant = json::array();
    for (auto& u : config::lib_uses(p.dir, false))
        rant.push_back((u.version ? *u.version : "none") + " through " + u.how + " in " + ctx.shown(u.file));
    j["rant"] = rant;
    config::Build b = config::plan_build(ctx.cwd, { p.name });
    json depends = json::array(), build = json::array();
    for (auto& e : b.edges)
        if (e.from == p.name) depends.push_back(e.to + " (" + e.source + ")");
    for (auto& st : b.steps) {
        if (st.package != p.name) continue;
        if (!st.configure.empty()) build.push_back(process::shown(st.configure) + " (asks first)");
        for (auto& c : st.commands) build.push_back(process::shown(c));
    }
    j["depends"] = depends;
    j["build"] = build;
    json types = json::array();
    for (auto& n : p.nodes) types.push_back(n.ref());
    j["types"] = types;
    return a;
}

static Answer type_answer(app::Context& ctx, const config::Workspace& ws, const state::State& st, const config::NodeType& n) {
    Answer a;
    json& j = a.fields;
    j["type"] = n.ref();
    j["kind"] = config::kind_name(n.kind);
    if (n.path) j["file"] = ctx.shown(*n.path);
    j["run"] = process::shown(n.run);
    j["cwd"] = ctx.shown(n.cwd);
    json groups = json::array(), running = json::array();
    for (auto& g : ws.groups) {
        config::Plan plan = config::plan_group(ctx.cwd, g.name, {});
        if (std::any_of(plan.instances.begin(), plan.instances.end(), [&](const config::Instance& i) { return i.type == type_of(n); }))
            groups.push_back(g.name);
    }
    for (auto& i : st.instances)
        if (i.type == type_of(n)) running.push_back(i.name);
    j["groups"] = groups;
    j["running"] = running;
    return a;
}

static Answer group_answer(app::Context& ctx, const state::State& st, const config::GroupInfo& g) {
    Answer a;
    json& j = a.fields;
    j["group"] = g.name;
    j["file"] = ctx.shown(g.file);
    if (!g.description.empty()) j["description"] = g.description;
    json params = json::object();
    for (auto& p : g.params) {
        std::string about = p.type + (p.default_value ? ", default " + *p.default_value : ", required");
        if (!p.options.empty()) {
            about += ", one of";
            for (size_t i = 0; i < p.options.size(); i++) about += (i ? ", " : " ") + p.options[i];
        }
        params[p.name] = about;
    }
    j["params"] = params;
    config::Plan plan = config::plan_group(ctx.cwd, g.name, {});
    if (plan.diagnostics.empty()) {
        json nodes = json::object();
        for (auto& i : plan.instances) nodes[i.name] = i.type;
        j["nodes"] = nodes;
    } else {
        j["nodes"] = "set its required params to see them";
    }
    json running = json::array();
    for (auto& r : st.roots)
        if (r.kind == "group" && r.name == g.name) running.push_back(r.key().substr(6));
    j["running"] = running;
    return a;
}

/* What the workspace has by that name: packages by name or folder, node types by reference
 * or bare name, and the group a reference finds. */
static void workspace_answers(app::Context& ctx, std::optional<Kind> kind, const std::string& name, const state::State& st,
                              std::vector<std::pair<Kind, Answer>>& out) {
    const config::Workspace* ws = ctx.workspace();
    if (!ws) return;
    const config::Workspace& full = ctx.require_workspace(true);
    auto want = [&](Kind k) { return !kind || *kind == k; };
    if (want(Kind::Package)) {
        std::error_code ec;
        fs::path folder = fs::weakly_canonical(ctx.cwd / config::from_utf8(name), ec);
        for (auto& p : full.packages)
            if (p.name == name || (!ec && fs::equivalent(p.dir, folder, ec))) out.push_back({ Kind::Package, package_answer(ctx, p) });
    }
    if (want(Kind::Type))
        for (auto* n : all_types(full))
            if (n->ref() == name || n->name == name) out.push_back({ Kind::Type, type_answer(ctx, full, st, *n) });
    if (want(Kind::Group)) {
        config::GroupInfo g = config::describe_group(ctx.cwd, name);
        if (g.diagnostics.empty()) out.push_back({ Kind::Group, group_answer(ctx, st, g) });
    }
}

static void mesh_answers(app::Context& ctx, std::optional<Kind> kind, const std::string& name, const state::State& st,
                         std::vector<std::pair<Kind, Answer>>& out) {
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    std::vector<mesh::Peer> peers = mesh.peers();    /* outlives the views that point into it */
    if (!kind || *kind == Kind::Node)
        for (auto& v : find_nodes(mesh, st, peers, name)) out.push_back({ Kind::Node, node_answer(mesh, v) });
    for (auto& e : mesh.entities())
        if (e.name == name && (!kind || entity_matches(*kind, mesh::kind_name(e.kind))))
            out.push_back({ Kind::Entity, entity_answer(mesh, e) });
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    std::optional<Kind> kind;
    if (w.size() == 2 && !(kind = kind_named(w[0])))
        throw app::UsageError("`" + w[0] + "` is no kind, use node, entity, topic, var, fn, task, package, type or group");
    if (w.empty() || w.size() > 2) throw app::UsageError("info takes a name, or a kind and a name such as `rant info package cam`");
    const std::string& name = w.back();

    state::State st = run::snapshot(ctx);
    std::vector<std::pair<Kind, Answer>> answers;
    if (!kind || !on_mesh(*kind)) workspace_answers(ctx, kind, name, st, answers);
    if (!kind || on_mesh(*kind)) mesh_answers(ctx, kind, name, st, answers);
    if (answers.empty())
        throw app::Failure(kind ? "no " + std::string(kind_word(*kind)) + " named `" + name + "`, see `rant ls`"
                                : "nothing named `" + name + "`, see `rant ls`");

    std::vector<std::string> kinds;
    for (auto& [k, _] : answers)
        if (std::find(kinds.begin(), kinds.end(), kind_word(k)) == kinds.end()) kinds.push_back(kind_word(k));
    if (kinds.size() > 1) {
        std::string list;
        for (size_t i = 0; i < kinds.size(); i++) {
            const char* article = std::string("aeiou").find(kinds[i][0]) == std::string::npos ? "a " : "an ";
            list += (i == 0 ? "" : i + 1 == kinds.size() ? " and " : ", ") + std::string(article) + kinds[i];
        }
        ctx.out.note("`" + name + "` names " + list + ", `rant info " + kinds[0] + " " + name + "` shows only one");
    }

    if (ctx.json) {
        json out = json::array();
        for (auto& [_, a] : answers) out.push_back(a.fields);
        ctx.out.line(out.dump(2, ' ', false, json::error_handler_t::replace));
        return 0;
    }
    for (size_t i = 0; i < answers.size(); i++) {
        if (i) ctx.out.line();
        print_answer(ctx, answers[i].second);
    }
    return 0;
}

/* Names of a kind, or of every kind, for completion. */
static std::vector<std::string> names(complete::Request& r, std::optional<Kind> kind) {
    std::vector<std::string> out;
    auto add = [&](std::vector<std::string> v) { out.insert(out.end(), v.begin(), v.end()); };
    auto want = [&](Kind k) { return !kind || *kind == k; };
    if (want(Kind::Package)) add(r.packages());
    if (want(Kind::Type)) add(r.node_types());
    if (want(Kind::Group)) add(r.groups());
    if (want(Kind::Node)) {
        add(r.mesh_nodes());
        add(r.running_nodes());
    }
    if (!kind) add(r.entities());
    else if (*kind == Kind::Topic) add(r.entities({ rant::EntityKind::Topic }));
    else if (*kind == Kind::Variable) add(r.entities({ rant::EntityKind::Variable }));
    else if (*kind == Kind::Function) add(r.entities({ rant::EntityKind::Function }));
    else if (*kind == Kind::Task) add(r.entities({ rant::EntityKind::Task }));
    else if (*kind == Kind::Entity) add(r.entities());
    return out;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) {
        std::vector<std::string> out = kind_words(false);
        for (auto& n : names(r, std::nullopt)) out.push_back(n);
        return { out, true };
    }
    if (r.words.size() == 1)
        if (auto k = kind_named(r.words[0])) return { names(r, k), true };
    return {};
}

app::Command info() {
    app::Command c{ "info", "[kind] <name>", "explain a node, entity, package, node type or group", app::Section::Mesh, {}, run };
    c.complete = complete_words;
    return c;
}

}
