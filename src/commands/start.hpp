#pragma once

#include <string>
#include <vector>

#include "app/context.hpp"

/* What `rant start node` and `rant start group` do, shared with restart, which starts what
 * is not running. */
namespace commands {

int start_node(app::Context& ctx, const std::string& node);
int start_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);

}
