#include "commands/kinds.hpp"

namespace commands {

struct Words {
    Kind        kind;
    const char* singular;
    const char* plural;
    std::vector<std::string_view> more;    /* other words a user may type */
};

static const std::vector<Words>& table() {
    static const std::vector<Words> t = {
        { Kind::Node, "node", "nodes", {} },
        { Kind::Entity, "entity", "entities", {} },
        { Kind::Topic, "topic", "topics", {} },
        { Kind::Variable, "var", "vars", { "variable", "variables" } },
        { Kind::Function, "fn", "fns", { "function", "functions" } },
        { Kind::Task, "task", "tasks", {} },
        { Kind::Package, "package", "packages", { "pkg", "pkgs" } },
        { Kind::Type, "type", "types", {} },
        { Kind::Group, "group", "groups", {} },
    };
    return t;
}

std::optional<Kind> kind_named(std::string_view word) {
    for (auto& w : table()) {
        if (word == w.singular || word == w.plural) return w.kind;
        for (auto m : w.more)
            if (word == m) return w.kind;
    }
    return std::nullopt;
}

const char* kind_word(Kind k) {
    for (auto& w : table())
        if (w.kind == k) return w.singular;
    return "?";
}

std::vector<std::string> kind_words(bool plural) {
    std::vector<std::string> out;
    for (auto& w : table()) out.emplace_back(plural ? w.plural : w.singular);
    return out;
}

bool on_mesh(Kind k) { return k != Kind::Package && k != Kind::Type && k != Kind::Group; }

bool entity_matches(Kind k, std::string_view entity_kind) {
    return k == Kind::Entity || (on_mesh(k) && k != Kind::Node && entity_kind == kind_word(k));
}

}
