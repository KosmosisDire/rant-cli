#pragma once

#include <string>

#include "rant.hpp"
#include "ui/output.hpp"

namespace mesh {

/* A type as schema text, one field per line and colored when out paints. A standard type
 * is its name alone. A user type comes first, then each user type it uses, while the
 * standard types it uses stay names. "untyped" when there is no schema. */
std::string schema_text(const rant::Schema& s, const ui::Output* out);

}
