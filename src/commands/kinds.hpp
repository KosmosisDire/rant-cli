#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace commands {

/* The kinds of thing ls lists and info explains. Topic, Variable, Function and Task narrow
 * Entity. A node is a running one or one that could start. */
enum class Kind { Node, Entity, Topic, Variable, Function, Task, Package, Group };

/* The kind a word names, singular or plural, short or long: node, nodes, pkg, packages. */
std::optional<Kind> kind_named(std::string_view word);

/* The word for a kind in messages and in info: "node", "package". */
const char* kind_word(Kind k);

/* What ls offers after it, plural, and info, singular, for completion. */
std::vector<std::string> kind_words(bool plural);

bool on_mesh(Kind k);

/* An entity kind matches k when k is Entity or names that kind. */
bool entity_matches(Kind k, std::string_view entity_kind);

}
