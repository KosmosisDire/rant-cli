#pragma once

#include <nlohmann/json.hpp>

#include "rant.hpp"

/* Messages as JSON, walked through their schema at run time, so the CLI reads and writes
 * any type without compiling it in. */
namespace mesh {

using json = nlohmann::ordered_json;

/* A message as JSON: an object for a struct, the bare value for a bare root. Bytes with
 * no schema become a "0x..." hex string. */
json to_json(rant::Bytes data, const rant::Schema& schema);

/* The compact one line form printed by sub, get and call. */
std::string to_text(rant::Bytes data, const rant::Schema& schema);

}
