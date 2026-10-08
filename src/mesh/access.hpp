#pragma once

#include <chrono>
#include <optional>
#include <string>
#include <vector>

#include "mesh/client.hpp"
#include "mesh/values.hpp"

/* Reading and writing entities by name, each adopting the entity's schema from the mesh. */
namespace mesh {

/* The entity of one of kinds by that name on the mesh. Throws app::Failure naming what
 * is there instead, such as a topic where a variable was asked for. */
rant::Entity require_entity(Client& c, const std::vector<rant::EntityKind>& kinds, const std::string& name);

/* Why a send, set or call did not go out, in a reader's words. */
std::string send_failure(rant::SendStatus s);

/* A variable's current value, or nullopt when no owner answered within limit. */
std::optional<json> read_variable(Client& c, const std::string& name,
                                  std::chrono::milliseconds limit = std::chrono::milliseconds(2000));

}
