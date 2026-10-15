#pragma once

#include <map>
#include <string>
#include <vector>

#include "rant.hpp"
#include "ui/output.hpp"

namespace mesh {

/* A type as schema text, one field per line and colored when out paints. A standard type
 * is its name alone unless whole spells it out too. A user type comes first, then each user
 * type it uses, while the standard types it uses stay names. "untyped" when there is no
 * schema. */
std::string schema_text(const rant::Schema& s, const ui::Output* out, bool whole = false);

/* A named type the mesh uses, with the entities that use it. */
struct TypeUse {
    rant::Schema             schema;
    std::vector<std::string> entities;
};

/* Every named type of the entities' messages, requests, responses and progress, by name. */
std::map<std::string, TypeUse> types_in_use(const std::vector<rant::Entity>& entities);

/* The names of every standard type. */
std::vector<std::string> standard_types();

}
