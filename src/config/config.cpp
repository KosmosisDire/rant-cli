#include "config/config.hpp"

#include <memory>

#include "rant_config.h"

namespace config {

std::string Diagnostic::str() const {
    std::string s = file;
    if (line) s += ":" + std::to_string(line) + ":" + std::to_string(column);
    return s + ": " + message;
}

fs::path from_utf8(const std::string& s) { return fs::u8path(s); }
std::string to_utf8(const fs::path& p) { return p.generic_u8string(); }

static std::string str(const char* s) { return s ? std::string(s) : std::string(); }

static std::vector<Diagnostic> diagnostics(const RantConfigDiagnostic* d, size_t n) {
    std::vector<Diagnostic> out;
    for (size_t i = 0; i < n; i++) out.push_back({ str(d[i].file), d[i].line, d[i].column, str(d[i].message) });
    return out;
}

using Handle = std::unique_ptr<RantConfigWorkspace, decltype(&rant_config_free)>;

static Opened read(Handle h) {
    const RantConfigWorkspaceView* v = rant_config_view(h.get());
    Opened out;
    out.diagnostics = diagnostics(v->diagnostics, v->diagnostic_count);
    if (v->root) out.workspace = Workspace{ from_utf8(v->root), from_utf8(str(v->logs)) };
    return out;
}

Opened open(const fs::path& start) {
    return read(Handle(rant_config_open(to_utf8(start).c_str()), &rant_config_free));
}

Opened init(const fs::path& dir) {
    return read(Handle(rant_config_init(to_utf8(dir).c_str()), &rant_config_free));
}

}
