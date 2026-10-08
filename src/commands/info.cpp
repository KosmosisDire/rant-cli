#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/access.hpp"
#include "mesh/schema_text.hpp"
#include "run/nodes.hpp"
#include "ui/yaml.hpp"

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

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("info takes one name");
    const std::string& name = ctx.args.words[0];

    state::State st = run::snapshot(ctx);
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    std::vector<mesh::Peer> peers = mesh.peers();

    std::vector<Answer> answers;
    for (auto& v : find_nodes(mesh, st, peers, name)) answers.push_back(node_answer(mesh, v));
    for (auto& e : mesh.entities())
        if (e.name == name) answers.push_back(entity_answer(mesh, e));
    if (answers.empty()) throw app::Failure("nothing named `" + name + "` is running, see `rant ls`");

    if (ctx.json) {
        json out = json::array();
        for (auto& a : answers) out.push_back(a.fields);
        ctx.out.line(out.dump(2, ' ', false, json::error_handler_t::replace));
        return 0;
    }
    for (size_t i = 0; i < answers.size(); i++) {
        if (i) ctx.out.line();
        print_answer(ctx, answers[i]);
    }
    return 0;
}

app::Command info() {
    return { "info", "<name>", "explain one node or entity", app::Section::Mesh, {}, run };
}

}
