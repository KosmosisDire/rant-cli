#pragma once

#include <functional>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "app/context.hpp"
#include "state/state.hpp"

/* What `rant run` does, shared with restart, which runs what is not running, and with stop. */
namespace commands {

int run_node(app::Context& ctx, const std::string& node);
int run_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);

/* A group as the workspace names it in full, such as drivers/Base, or the word itself
 * when it names no group. */
std::string group_name(app::Context& ctx, const std::string& group);

/* Whether stop, restart and log take a bare name as a group: a group of it that runs, else no
 * node of it that runs and a group of it in the config, so the one running wins. */
bool names_group(app::Context& ctx, const std::string& name);

/* Every state on this machine rant started nodes in, this workspace's first, with the folder it
 * serves. With prune each waits for its lock and drops what is gone, else it only reads. */
std::vector<std::pair<config::fs::path, state::State>> states_anywhere(app::Context& ctx, bool prune);

/* Runs act once in each folder, as if rant ran there, given the folder as shown when it is
 * not the one rant runs in. The first failing code is kept. */
int in_each(app::Context& ctx, const std::vector<config::fs::path>& roots, const std::function<int(const std::string&)>& act);

/* The folders on this machine whose rant runs a bare name, for stop, restart and log to act
 * in: those running a group of it, else those running a node of it, with node only nodes.
 * Several are narrowed by a pick, or a failure naming them when no one can answer. Empty when
 * it runs nowhere, nullopt when the pick was given up. */
std::optional<std::vector<config::fs::path>> where_running(app::Context& ctx, const std::string& name, bool node,
                                                           const std::string& verb);

/* A group's key=value words with each path param made whole from here, so a run planned from
 * another folder means the same. Words the group cannot plan stay, for the plan to refuse. */
std::vector<std::string> resolved_params(app::Context& ctx, const std::string& group, std::vector<std::string> words);

/* What `rant stop` does to a group, with params to pick one of its runs, and to one node by
 * name, shared with disable. */
int stop_group(app::Context& ctx, const std::string& group, const std::vector<std::string>& params);
int stop_node(app::Context& ctx, const std::string& name);

}
