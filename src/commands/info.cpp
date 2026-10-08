#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/access.hpp"

namespace commands {

using mesh::json;

/* What a node does with an entity, in the words a reader expects for its kind. */
static std::vector<std::string> roles(const rant::Entity& e) {
    std::vector<std::string> out;
    bool topic = e.kind == rant::EntityKind::Topic, var = e.kind == rant::EntityKind::Variable;
    if (e.provides) out.push_back(topic ? "publishes" : var ? "owns" : "serves");
    if (e.consumes) out.push_back(topic ? "subscribes" : var ? "reads" : "calls");
    return out;
}

static std::string joined(const std::vector<std::string>& v) {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : ", ") + x;
    return s.empty() ? "-" : s;
}

static void row(app::Context& ctx, json& j, const std::string& key, const json& value, const std::string& shown) {
    j[key] = value;
    if (ctx.json) return;
    std::string k = key;
    k.resize(12, ' ');
    ctx.out.line("  " + ctx.out.paint(ui::Style::Dim, k) + shown);
}

static json node_info(app::Context& ctx, mesh::Client& mesh, const mesh::Peer& p) {
    json j = json::object();
    if (!ctx.json) ctx.out.line(ctx.out.paint(ui::Style::Bold, "node " + p.name));
    row(ctx, j, "address", p.address, p.address);
    auto pids = mesh.pids({ p.id });
    if (pids.count(p.id)) row(ctx, j, "pid", pids[p.id], std::to_string(pids[p.id]));
    json uses = json::array();
    std::vector<std::string> lines;
    for (auto& e : mesh.entities_of(p.id)) {
        for (auto& r : roles(e)) {
            uses.push_back({ { "name", e.name }, { "kind", mesh::kind_name(e.kind) }, { "role", r } });
            std::string role = r;
            role.resize(11, ' ');
            lines.push_back(role + e.name + " " + ctx.out.paint(ui::Style::Dim, mesh::kind_name(e.kind)));
        }
    }
    j["entities"] = uses;
    if (!ctx.json)
        for (size_t i = 0; i < lines.size(); i++)
            ctx.out.line("  " + ctx.out.paint(ui::Style::Dim, i ? "            " : "entities    ") + lines[i]);
    return j;
}

static json entity_info(app::Context& ctx, mesh::Client& mesh, const rant::Entity& e) {
    json j = json::object();
    if (!ctx.json) ctx.out.line(ctx.out.paint(ui::Style::Bold, std::string(mesh::kind_name(e.kind)) + " " + e.name));
    j["kind"] = mesh::kind_name(e.kind);
    bool call = e.kind == rant::EntityKind::Function || e.kind == rant::EntityKind::Task;
    if (call) {
        row(ctx, j, "request", mesh::type_text(e.schema), mesh::type_text(e.schema));
        row(ctx, j, "response", mesh::type_text(e.rsp_schema), mesh::type_text(e.rsp_schema));
        if (e.kind == rant::EntityKind::Task)
            row(ctx, j, "progress", mesh::type_text(e.progress_schema), mesh::type_text(e.progress_schema));
    } else {
        row(ctx, j, "type", mesh::type_text(e.schema), mesh::type_text(e.schema));
    }

    std::vector<std::string> providers, consumers;
    for (auto& p : mesh.peers())
        for (auto& pe : mesh.entities_of(p.id))
            if (pe.kind == e.kind && pe.name == e.name) {
                if (pe.provides) providers.push_back(p.name);
                if (pe.consumes) consumers.push_back(p.name);
            }
    bool topic = e.kind == rant::EntityKind::Topic, var = e.kind == rant::EntityKind::Variable;
    row(ctx, j, topic ? "publishers" : var ? "owner" : "providers", providers, joined(providers));
    row(ctx, j, topic ? "subscribers" : var ? "readers" : "callers", consumers, joined(consumers));

    if (var) {
        auto value = mesh::read_variable(mesh, e.name);
        row(ctx, j, "value", value ? *value : json(nullptr),
            value ? value->dump(-1, ' ', false, json::error_handler_t::replace) : "(no value)");
    }
    return j;
}

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("info takes one name");
    const std::string& name = ctx.args.words[0];

    mesh::Client mesh(ctx.domain);
    mesh.settle();

    json out = json::array();
    for (auto& p : mesh.peers()) {
        if (p.name != name) continue;
        if (!ctx.json && !out.empty()) ctx.out.line();
        json j = node_info(ctx, mesh, p);
        j["node"] = p.name;
        out.push_back(j);
    }
    for (auto& e : mesh.entities()) {
        if (e.name != name) continue;
        if (!ctx.json && !out.empty()) ctx.out.line();
        json j = entity_info(ctx, mesh, e);
        j["entity"] = e.name;
        out.push_back(j);
    }
    if (out.empty()) throw app::Failure("nothing named `" + name + "` is on the mesh, see `rant ls`");
    if (ctx.json) ctx.out.line(out.dump(2));
    return 0;
}

app::Command info() {
    return { "info", "<name>", "explain one node or entity", app::Section::Mesh, {}, run };
}

}
