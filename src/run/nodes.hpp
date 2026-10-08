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
 * last run's log kept as <name>.log.1. Returns the record for the state, with no roots. */
state::Instance spawn(const config::Workspace& ws, const config::Instance& inst, const std::string& name);

/* Stops an instance and everything it started: gracefully, then by force after grace. */
process::Stopped halt(const state::Instance& inst);

/* A running instance is the plan's node: same type, argv, env and working directory. */
bool same_spec(const state::Instance& running, const config::Instance& planned);

/* What releasing a root did to one of its nodes. */
struct Released {
    std::string              name;
    std::vector<std::string> kept_for;    /* the roots still needing it, empty when stopped */
    process::Stopped         how = process::Stopped::AlreadyGone;
};

/* Drops a root. Each of its nodes no other root needs stops, last started first, and a
 * node another root still needs keeps running. */
std::vector<Released> release_root(Session& s, const std::string& key);

/* The live mesh peers that are instances rant started, by peer id, each with the instance
 * name. A peer belongs to an instance when the process id it reports is in the instance's
 * process group or job, so a node behind a wrapper still matches. */
std::map<uint32_t, std::string> managed_peers(mesh::Client& mesh, const state::State& st);

}
