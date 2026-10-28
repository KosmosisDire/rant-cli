#pragma once

#include <set>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "state/state.hpp"

namespace commands {

/* `rant enable` with these words, such as {"Bridge"} or {"node", "talker"}. */
int enable_named(app::Context& ctx, const std::vector<std::string>& words);

/* `rant ls services`: what rant enable named, whether it runs and how often its nodes were
 * started again. */
int list_services(app::Context& ctx, const std::string& pattern);

/* What a stop or restart of everything leaves be in one workspace, since only naming a service
 * stops it: the roots its enabled groups and nodes run under, the nodes they hold, and the
 * running ones as rant ls services names them. */
struct Kept {
    std::set<std::string>    roots;
    std::set<std::string>    nodes;
    std::vector<std::string> running;
};

/* st is the state of the workspace whose data folder is data. */
Kept kept_services(const state::State& st, const config::fs::path& data);

}
