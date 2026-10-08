#include "config/config.hpp"

#include <memory>

#include "rant_config.h"

namespace config {

std::string Diagnostic::str() const {
    if (file.empty()) return message;
    std::string s = file;
    if (line) s += ":" + std::to_string(line) + ":" + std::to_string(column);
    return s + ": " + message;
}

const char* kind_name(NodeKind k) {
    switch (k) {
    case NodeKind::Native:   return "native";
    case NodeKind::Python:   return "python";
    case NodeKind::CSharp:   return "csharp";
    case NodeKind::Declared: return "declared";
    }
    return "?";
}

fs::path from_utf8(const std::string& s) { return fs::u8path(s); }
std::string to_utf8(const fs::path& p) { return p.generic_u8string(); }

static std::string str(const char* s) { return s ? std::string(s) : std::string(); }

static std::vector<std::string> strs(const char* const* v, size_t n) {
    std::vector<std::string> out;
    for (size_t i = 0; i < n; i++) out.push_back(str(v[i]));
    return out;
}

static std::vector<Diagnostic> diagnostics(const RantConfigDiagnostic* d, size_t n) {
    std::vector<Diagnostic> out;
    for (size_t i = 0; i < n; i++) out.push_back({ str(d[i].file), d[i].line, d[i].column, str(d[i].message) });
    return out;
}

static NodeType node_type(const RantConfigNodeType& n) {
    NodeType t;
    t.package = str(n.package);
    t.name = str(n.name);
    t.kind = static_cast<NodeKind>(n.kind);
    if (n.path) t.path = from_utf8(n.path);
    t.run = strs(n.run, n.run_count);
    t.cwd = from_utf8(str(n.cwd));
    return t;
}

using Handle = std::unique_ptr<RantConfigWorkspace, decltype(&rant_config_free)>;

/* A workspace with its paths only, packages and groups left empty. */
static Workspace paths_only(const char* root, const char* logs, const char* data) {
    Workspace ws;
    ws.root = from_utf8(root);
    ws.logs = from_utf8(str(logs));
    ws.data = from_utf8(str(data));
    return ws;
}

static Opened read(Handle h) {
    const RantConfigWorkspaceView* v = rant_config_view(h.get());
    Opened out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    if (!v->root) return out;
    Workspace ws = paths_only(v->root, v->logs, v->data);
    for (size_t i = 0; i < v->package_count; i++) {
        const RantConfigPackage& p = v->packages[i];
        Package pkg{ str(p.name), from_utf8(str(p.dir)), strs(p.kinds, p.kind_count), {} };
        for (size_t j = 0; j < p.node_count; j++) pkg.nodes.push_back(node_type(p.nodes[j]));
        ws.packages.push_back(std::move(pkg));
    }
    for (size_t i = 0; i < v->loose_count; i++) ws.loose.push_back(node_type(v->loose[i]));
    for (size_t i = 0; i < v->group_count; i++)
        ws.groups.push_back({ str(v->groups[i].name), from_utf8(str(v->groups[i].file)) });
    out.workspace = std::move(ws);
    return out;
}

static Plan read_plan(const RantConfigPlanView* v) {
    Plan out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    if (v->root) out.workspace = paths_only(v->root, v->logs, v->data);
    out.group = str(v->group);
    for (size_t i = 0; i < v->param_count; i++) out.params[str(v->params[i].name)] = str(v->params[i].value);
    for (size_t i = 0; i < v->instance_count; i++) {
        const RantConfigInstance& in = v->instances[i];
        Instance inst;
        inst.name = str(in.name);
        inst.type = str(in.type_ref);
        inst.kind = static_cast<NodeKind>(in.kind);
        inst.argv = strs(in.argv, in.argc);
        for (size_t e = 0; e < in.env_count; e++) inst.env[str(in.env[e].name)] = str(in.env[e].value);
        inst.cwd = from_utf8(str(in.cwd));
        out.instances.push_back(std::move(inst));
    }
    return out;
}

using PlanHandle = std::unique_ptr<RantConfigPlan, decltype(&rant_config_plan_free)>;

Plan plan_node(const fs::path& start, const std::string& node_type) {
    PlanHandle h(rant_config_plan_node(to_utf8(start).c_str(), node_type.c_str()), &rant_config_plan_free);
    return read_plan(rant_config_plan_view(h.get()));
}

Plan plan_group(const fs::path& start, const std::string& group, const std::vector<std::string>& params) {
    std::vector<const char*> raw;
    for (auto& p : params) raw.push_back(p.c_str());
    PlanHandle h(rant_config_plan_group(to_utf8(start).c_str(), group.c_str(), raw.data(), raw.size()),
                 &rant_config_plan_free);
    return read_plan(rant_config_plan_view(h.get()));
}

static std::vector<Param> params_of(const RantConfigParam* ps, size_t n) {
    std::vector<Param> out;
    for (size_t i = 0; i < n; i++) {
        const RantConfigParam& p = ps[i];
        Param param{ str(p.name), str(p.type_name), std::nullopt, strs(p.options, p.option_count), str(p.description) };
        if (p.default_value) param.default_value = p.default_value;
        out.push_back(std::move(param));
    }
    return out;
}

GroupInfo describe_group(const fs::path& start, const std::string& group) {
    std::unique_ptr<RantConfigGroup, decltype(&rant_config_group_free)> h(
        rant_config_group(to_utf8(start).c_str(), group.c_str()), &rant_config_group_free);
    const RantConfigGroupView* v = rant_config_group_view(h.get());
    GroupInfo out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    out.name = str(v->name);
    if (v->file) out.file = from_utf8(v->file);
    out.description = str(v->description);
    out.params = params_of(v->params, v->param_count);
    return out;
}

TemplateManifest read_template(const std::string& text, const fs::path& path, const std::vector<std::string>& params, bool bind) {
    std::vector<const char*> raw;
    for (auto& p : params) raw.push_back(p.c_str());
    std::string where = to_utf8(path);
    std::unique_ptr<RantConfigTemplate, decltype(&rant_config_template_free)> h(
        rant_config_template(text.c_str(), where.c_str(), raw.data(), raw.size(), bind), &rant_config_template_free);
    const RantConfigTemplateView* v = rant_config_template_view(h.get());
    TemplateManifest out;
    out.description = str(v->description);
    out.includes = strs(v->includes, v->include_count);
    out.next = str(v->next);
    out.params = params_of(v->params, v->param_count);
    out.values = str(v->values);
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    return out;
}

static std::vector<std::string> command(const RantConfigCommand& c) { return strs(c.argv, c.argc); }

Build plan_build(const fs::path& start, const std::vector<std::string>& packages) {
    std::vector<const char*> raw;
    for (auto& p : packages) raw.push_back(p.c_str());
    std::unique_ptr<RantConfigBuild, decltype(&rant_config_build_free)> h(
        rant_config_build(to_utf8(start).c_str(), raw.data(), raw.size()), &rant_config_build_free);
    const RantConfigBuildView* v = rant_config_build_view(h.get());
    Build out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    for (size_t i = 0; i < v->step_count; i++) {
        const RantConfigBuildStep& s = v->steps[i];
        BuildStep step{ str(s.package), from_utf8(str(s.dir)), command(s.configure), {} };
        for (size_t j = 0; j < s.command_count; j++) step.commands.push_back(command(s.commands[j]));
        out.steps.push_back(std::move(step));
    }
    for (size_t i = 0; i < v->edge_count; i++)
        out.edges.push_back({ str(v->edges[i].from), str(v->edges[i].to), str(v->edges[i].source) });
    return out;
}

PythonInfo python_of(const fs::path& dir) {
    std::unique_ptr<RantConfigPython, decltype(&rant_config_python_free)> h(rant_config_python(to_utf8(dir).c_str()),
                                                                           &rant_config_python_free);
    const RantConfigPythonView* v = rant_config_python_view(h.get());
    PythonInfo out;
    if (v->venv) out.venv = from_utf8(v->venv);
    out.venv_python = from_utf8(str(v->venv_python));
    out.interpreter = strs(v->interpreter, v->interpreter_count);
    out.imports_rant = v->imports_rant;
    if (v->root) out.root = from_utf8(v->root);
    return out;
}

Opened open(const fs::path& start, bool packages) {
    return read(Handle(rant_config_open(to_utf8(start).c_str(), packages ? RANT_CONFIG_PACKAGES : 0), &rant_config_free));
}

Opened init(const fs::path& dir) {
    return read(Handle(rant_config_init(to_utf8(dir).c_str()), &rant_config_free));
}

}
