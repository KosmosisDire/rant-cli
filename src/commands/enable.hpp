#pragma once

#include <set>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "state/state.hpp"

namespace commands {

/* `rant enable` with these words: a group with its key=value params to add to the services
 * first, such as {"Bridge"}, or none for the services as they are. */
int enable_named(app::Context& ctx, const std::vector<std::string>& words);

/* `rant ls services`: the services of each enabled workspace, and of this one while it is not
 * enabled, whether they run and how often their nodes were started again. */
int list_services(app::Context& ctx, const std::string& pattern);

/* What a stop or restart of everything leaves be in one workspace, since only naming a service
 * stops it: when the workspace is enabled, the roots its services run under, the nodes they
 * hold, and the running ones as rant ls services names them. */
struct Kept {
    std::set<std::string>    roots;
    std::set<std::string>    nodes;
    std::vector<std::string> running;
};

/* st is the state of the workspace whose data folder is data. */
Kept kept_services(const state::State& st, const config::fs::path& data);

}
