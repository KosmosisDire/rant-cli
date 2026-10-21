#pragma once

#include <chrono>
#include <cstdint>
#include <map>
#include <string>

#include "app/context.hpp"
#include "config/config.hpp"
#include "mesh/client.hpp"
#include "state/state.hpp"

/* Running nodes: what start and stop share. Spawning a plan instance into the state,
 * stopping one, and matching the mesh's nodes to the instances rant started. */
namespace run {

/* How long a node gets to stop on its own before it is killed. */
constexpr std::chrono::seconds grace{ 3 };

/* The workspace's state, locked and pruned of dead instances, for a command that acts. */
class Session {
public:
    explicit Session(const config::Workspace& ws);

    const config::Workspace& workspace() const { return ws_; }
    state::State& state() { return store_.state(); }
    void save() { store_.save(); }

private:
    config::Workspace ws_;
    state::Store      store_;
};

/* The state as it stands, pruned and written back at once, so a command that only looks
 * holds the lock for a moment. Empty outside a workspace. */
state::State snapshot(app::Context& ctx);

/* Starts a plan instance under `name`, detached, its output in <logs>/<name>.log with the
 * last run's log kept as <name>.log.1. Returns the record for the state, with no roots. A
 * node with restart set is left to the service process, so its record has no process until
 * hand_over. */
state::Instance spawn(const config::Workspace& ws, const config::Instance& inst, const std::string& name);

/* Starts a record of the workspace whose data folder is data, as a child of this process,
 * its last log moved aside first. Throws app::Failure. */
process::Child start(const config::fs::path& data, const state::Instance& rec);

/* Once the state holding started is saved and unlocked: waits until the service process
 * started those spawn left to it, and returns them with their processes. Throws app::Failure. */
std::vector<state::Instance> hand_over(const config::Workspace& ws, std::vector<state::Instance> started);

/* An instance that exited, and how long after the watch began it was seen gone. */
struct Exit {
    const state::Instance*    instance;
    std::chrono::milliseconds after;
};

/* The instances of started that exited within span, sooner once all of them have. */
std::vector<Exit> exited_within(const std::vector<state::Instance>& started, std::chrono::milliseconds span);

/* Stops an instance and everything it started: gracefully, then by force after grace. */
process::Stopped halt(const state::Instance& inst);

/* A running instance is the plan's node: same type, argv, env and working directory. */
bool same_spec(const state::Instance& running, const config::Instance& planned);

/* The nodes a group's own node blocks made, in start order, whichever run started them: its
 * own, or another group's that includes it. */
std::vector<std::string> made_by(const state::State& st, const std::string& group);

/* Whether a group holds the instance. Its own start does not count, so the last group holding
 * a node stops it. */
bool held_by_group(const state::Instance& inst);

/* Drops a stopped instance from its state, with the node root that started it. */
void forget(state::State& st, const std::string& name);

/* What releasing a root did to one of its nodes. */
struct Released {
    std::string              name;
    std::vector<std::string> kept_for;    /* the roots still needing it, empty when stopped */
    process::Stopped         how = process::Stopped::AlreadyGone;
};

/* Drops a group's root. Each of its nodes no other group needs stops, last started first,
 * even one also started on its own, and a node another group still needs keeps running. */
std::vector<Released> release_root(Session& s, const std::string& key);

/* The live mesh peers that are instances rant started, by peer id, each with the instance
 * name. A peer belongs to an instance when the process id it reports is in the instance's
 * process group or job, so a node behind a wrapper still matches. */
std::map<uint32_t, std::string> managed_peers(mesh::Client& mesh, const state::State& st);

}
