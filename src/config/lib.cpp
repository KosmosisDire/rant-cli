#include "config/lib.hpp"

#include <memory>

#include "config/config.hpp"
#include "rant_config.h"

namespace config {

static std::string str(const char* s) { return s ? std::string(s) : std::string(); }

const char* lib_kind_name(LibKind k) {
    switch (k) {
    case LibKind::CMake:  return "cmake";
    case LibKind::Python: return "python";
    case LibKind::CSharp: return "csharp";
    }
    return "?";
}

std::vector<LibUse> lib_uses(const fs::path& path, bool recursive) {
    std::unique_ptr<RantConfigLibUses, decltype(&rant_config_lib_uses_free)> h(
        rant_config_lib_uses(to_utf8(path).c_str(), recursive), &rant_config_lib_uses_free);
    const RantConfigLibUsesView* v = rant_config_lib_uses_view(h.get());
    std::vector<LibUse> out;
    for (size_t i = 0; i < v->use_count; i++) {
        const RantConfigLibUse& u = v->uses[i];
        LibUse use;
        use.kind = static_cast<LibKind>(u.kind);
        use.how = str(u.how);
        use.dir = from_utf8(str(u.dir));
        use.file = from_utf8(str(u.file));
        if (u.version) use.version = u.version;
        use.venv_exists = u.venv_exists;
        if (u.venv_python) use.venv_python = from_utf8(u.venv_python);
        for (size_t j = 0; j < u.python_count; j++) use.python.push_back(str(u.python[j]));
        out.push_back(std::move(use));
    }
    return out;
}

using OutcomeHandle = std::unique_ptr<RantConfigOutcome, decltype(&rant_config_outcome_free)>;

static Outcome read(OutcomeHandle h) {
    const RantConfigOutcomeView* v = rant_config_outcome_view(h.get());
    return { str(v->error), str(v->note) };
}

Outcome lib_set(const fs::path& file, const std::string& version) {
    return read(OutcomeHandle(rant_config_lib_set(to_utf8(file).c_str(), version.c_str()), &rant_config_outcome_free));
}

Outcome lib_add(LibKind kind, const fs::path& file, const std::string& version) {
    return read(OutcomeHandle(rant_config_lib_add(static_cast<RantConfigLibKind>(kind), to_utf8(file).c_str(), version.c_str()),
                              &rant_config_outcome_free));
}

}
