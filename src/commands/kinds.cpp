#include "commands/kinds.hpp"

namespace commands {

struct Words {
    Kind        kind;
    const char* singular;
    const char* plural;
    std::vector<std::string_view> more;    /* other words a user may type */
    const char* listed;                    /* what ls lists for it */
};

static const std::vector<Words>& table() {
    static const std::vector<Words> t = {
        { Kind::Node, "node", "nodes", {}, "running nodes, and in a workspace the ones that could start" },
        { Kind::Entity, "entity", "entities", {}, "every topic, variable, function and task on the mesh" },
        { Kind::Topic, "topic", "topics", {}, "topics on the mesh" },
        { Kind::Variable, "var", "vars", { "variable", "variables" }, "variables on the mesh" },
        { Kind::Function, "fn", "fns", { "function", "functions" }, "functions on the mesh" },
        { Kind::Task, "task", "tasks", {}, "tasks on the mesh" },
        { Kind::Type, "type", "types", {}, "message types, the standard ones and those the mesh uses" },
        { Kind::Package, "package", "packages", { "pkg", "pkgs" }, "the packages of this workspace" },
        { Kind::Group, "group", "groups", {}, "the groups of this workspace" },
        { Kind::Source, "source", "sources", {}, "where rant install finds packages, in the order it looks" },
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
    for (auto& w : table())
        if (plural || explained(w.kind)) out.emplace_back(plural ? w.plural : w.singular);
    return out;
}

bool explained(Kind k) { return k != Kind::Source; }

std::vector<std::pair<std::string, std::string>> kinds_listed() {
    std::vector<std::pair<std::string, std::string>> out;
    for (auto& w : table()) out.emplace_back(w.plural, w.listed);
    return out;
}

bool on_mesh(Kind k) { return k != Kind::Package && k != Kind::Group && k != Kind::Source; }

bool entity_matches(Kind k, std::string_view entity_kind) {
    return k == Kind::Entity || (on_mesh(k) && k != Kind::Node && entity_kind == kind_word(k));
}

}
