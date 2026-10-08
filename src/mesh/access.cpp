#include "mesh/access.hpp"

#include <cctype>

namespace mesh {

std::string type_text(const rant::Schema& s) {
    if (s.empty()) return "untyped";
    std::string text = s.to_dsl(), out;
    bool space = false;
    for (char c : text) {
        if (std::isspace((unsigned char)c)) { space = !out.empty(); continue; }
        if (space) out += ' ';
        space = false;
        out += c;
    }
    return out;
}

std::optional<json> read_variable(Client& c, const std::string& name, std::chrono::milliseconds limit) {
    rant::VariableOptions<rant::Bytes> o;
    o.reflect_from_mesh = true;
    auto var = c.node().remote_variable<rant::Bytes>(name, o);
    if (!var.wait(limit)) return std::nullopt;
    auto bytes = var.get();
    if (!bytes) return std::nullopt;
    return to_json(rant::Bytes(bytes->data(), bytes->size()), var.schema());
}

}
