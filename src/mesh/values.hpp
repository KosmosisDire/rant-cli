#pragma once

#include <string>
#include <string_view>
#include <utility>
#include <vector>

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

/* One value typed on the command line, as JSON that forgives what a shell makes awkward:
 * keys and words need no quotes, so {x: 1, frame: map} and [a, b] both read. Throws
 * app::Failure saying where the text stopped making sense. */
json parse_text(std::string_view text);

/* What to write: the value each field path gets. A whole value has the path "". */
using Assignments = std::vector<std::pair<std::string, json>>;

/* Command line words as assignments: either one value, or `path=value` words such as
 * pose.position.x=1 or corners[2].y=3. */
Assignments parse_words(const std::vector<std::string>& words);

/* Builds a message of schema from the default message and the assignments, in order. An
 * unknown field or a value its field cannot hold throws app::Failure. With no schema the
 * one value goes as text. */
std::vector<uint8_t> encode(const Assignments& values, const rant::Schema& schema);

}
