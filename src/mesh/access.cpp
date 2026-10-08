#include "mesh/access.hpp"

#include <cctype>

#include "app/failure.hpp"

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

rant::Entity require_entity(Client& c, const std::vector<rant::EntityKind>& kinds, const std::string& name) {
    std::string other;
    for (auto& e : c.entities()) {
        if (e.name != name) continue;
        for (auto k : kinds)
            if (e.kind == k) return e;
        other = kind_name(e.kind);
    }
    std::string wanted;
    for (auto k : kinds) wanted += std::string(wanted.empty() ? "" : " or ") + kind_name(k);
    if (!other.empty()) throw app::Failure("`" + name + "` is a " + other + ", not a " + wanted);
    throw app::Failure("no " + wanted + " named `" + name + "` is on the mesh, see `rant ls entities`");
}

std::string send_failure(rant::SendStatus s) {
    switch (s) {
    case rant::SendStatus::Ok:          return "sent";
    case rant::SendStatus::NoTopic:     return "nobody on the other side answered in time";
    case rant::SendStatus::TooBig:      return "the value is too big";
    case rant::SendStatus::BadRole:     return "the other side does not take writes";
    case rant::SendStatus::OutOfMemory: return "out of memory";
    case rant::SendStatus::State:       return "refused right now";
    case rant::SendStatus::NoSys:       return "switched off on this node";
    case rant::SendStatus::Schema:      return "the value does not fit the type";
    }
    return "failed";
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
