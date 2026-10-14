#pragma once

#include <set>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "state/state.hpp"

/* What `rant run` does, shared with restart, which runs what is not running, and with stop. */
namespace commands {

int run_node(app::Context& ctx, const std::string& node);
int run_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);

/* Watches nodes just started for --watch seconds, 2 by default, since a node that crashes
 * at once, on an import error say, otherwise looks started. Each one that exited is named
 * with the end of its log and goes into exited. Returns 1 when one did, else 0. */
int watch_started(app::Context& ctx, const config::Workspace& ws, const std::vector<state::Instance>& started,
                  std::set<std::string>* exited = nullptr);

/* The --watch option of run and restart. */
app::OptionSpec watch_option();

/* Whether stop and restart take a name as a group: one the config defines or one running.
 * A name that is both a group and a node is a failure. */
bool names_group(app::Context& ctx, const std::string& name);

}
