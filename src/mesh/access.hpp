#pragma once

#include <chrono>
#include <optional>
#include <string>

#include "mesh/client.hpp"
#include "mesh/values.hpp"

/* Reading and writing entities by name, each adopting the entity's schema from the mesh. */
namespace mesh {

/* The type an entity carries, as one line of schema text, "untyped" when it has none. */
std::string type_text(const rant::Schema& s);

/* A variable's current value, or nullopt when no owner answered within limit. */
std::optional<json> read_variable(Client& c, const std::string& name,
                                  std::chrono::milliseconds limit = std::chrono::milliseconds(2000));

}
