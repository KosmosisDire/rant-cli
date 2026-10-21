#pragma once

#include <string>
#include <vector>

#include "app/context.hpp"

/* What `rant run` does, shared with restart, which runs what is not running, and with stop. */
namespace commands {

int run_node(app::Context& ctx, const std::string& node);
int run_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);

/* A group as the workspace names it in full, such as drivers/Base, or the word itself
 * when it names no group. */
std::string group_name(app::Context& ctx, const std::string& group);

/* Whether stop and restart take a name as a group: one the config defines or one running.
 * A name that is both a group and a node is a failure. */
bool names_group(app::Context& ctx, const std::string& name);

/* What `rant stop` does to a group, with params to pick one of its runs, and to one node by
 * name, shared with disable. */
int stop_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);
int stop_node(app::Context& ctx, const std::string& name);

}
