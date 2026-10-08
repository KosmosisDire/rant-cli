#pragma once

#include <string_view>

namespace util {

/* A name filter: a glob when the pattern holds * or ?, otherwise a substring. An empty
 * pattern matches everything. */
bool name_matches(std::string_view pattern, std::string_view name);

bool glob(std::string_view pattern, std::string_view text);

}
