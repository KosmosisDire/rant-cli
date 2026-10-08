#include "config/config.hpp"

#include <memory>

#include "rant_config.h"

namespace config {

std::string Diagnostic::str() const {
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

static Opened read(Handle h) {
    const RantConfigWorkspaceView* v = rant_config_view(h.get());
    Opened out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    if (!v->root) return out;
    Workspace ws{ from_utf8(v->root), from_utf8(str(v->logs)), from_utf8(str(v->data)), {} };
    for (size_t i = 0; i < v->package_count; i++) {
        const RantConfigPackage& p = v->packages[i];
        Package pkg{ str(p.name), from_utf8(str(p.dir)), {} };
        for (size_t j = 0; j < p.node_count; j++) pkg.nodes.push_back(node_type(p.nodes[j]));
        ws.packages.push_back(std::move(pkg));
    }
    out.workspace = std::move(ws);
    return out;
}

Opened open(const fs::path& start, bool packages) {
    return read(Handle(rant_config_open(to_utf8(start).c_str(), packages ? RANT_CONFIG_PACKAGES : 0), &rant_config_free));
}

Opened init(const fs::path& dir) {
    return read(Handle(rant_config_init(to_utf8(dir).c_str()), &rant_config_free));
}

}
