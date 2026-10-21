#pragma once

#include <filesystem>
#include <map>
#include <string>
#include <vector>

#include "process/supervisor.hpp"
#include "util/lock.hpp"

/* The workspace's record of what rant started: the roots asked for and the node instances
 * that satisfy them. No daemon keeps it: every command reads it, reconciles it against the
 * OS, acts and writes it back, all under one lock. */
namespace state {

namespace fs = std::filesystem;

/* Something started on purpose: a group with its params, or one node. */
struct Root {
    std::string                        kind;     /* "group" or "node" */
    std::string                        name;     /* the group, or the node instance */
    std::map<std::string, std::string> params;   /* a group's resolved params */

    /* "group nav target=red_bin" or "node talker": one per root. */
    std::string key() const;
};

/* One running node and how to find it again. */
struct Instance {
    std::string                        name;     /* its name on the mesh */
    std::string                        type;     /* the node type, package/name */
    std::vector<std::string>           argv;
    std::map<std::string, std::string> env;
    std::string                        cwd;
    std::string                        log;
    process::Tracking                  tracking;
    std::vector<std::string>           roots;    /* keys of the roots that need it, in order */
    std::string                        group;    /* the group whose own node block made it, empty for a node run alone */
    bool                               expect_mesh = true;    /* false for a program that never joins the mesh */
    std::string                        restart = "never";     /* or "on-failure" or "always", which the service process does */
    int                                restarts = 0;          /* how often the service process started it again */
    std::vector<std::string>           launch_env;            /* the environment of the command that left it to the service process */

    /* Same type, argv, env and working directory. */
    bool same_spec(const Instance& o) const;

    /* Its program or its working directory lies inside folder. */
    bool runs_from(const fs::path& folder) const;
};

struct State {
    std::vector<Root>     roots;
    std::vector<Instance> instances;    /* in start order */

    Instance* instance(const std::string& name);
    Root*     root(const std::string& key);
};

/* The state file, locked from construction to destruction so two commands never act on it
 * at once. A command blocked on the lock waits for the other to finish. Without wait it
 * gives up at once, and then locked() is false and the state empty. */
class Store {
public:
    explicit Store(const fs::path& data_dir, bool wait = true);
    Store(const Store&) = delete;
    Store& operator=(const Store&) = delete;

    bool   locked() const { return lock_.held(); }
    State& state() { return state_; }

    /* Writes a temp file and renames it into place, so a crash never leaves half a file. */
    void save();

private:
    fs::path       path_;
    util::FileLock lock_;
    State          state_;
};

/* Drops the instances whose processes are all gone, their names leave every root, and a
 * node root left with nothing goes too. Group roots stay, so starting the group again
 * brings back what crashed. restarting: the service process runs, so a node with restart
 * set stays for it, gone or not started yet. Returns the names dropped. */
std::vector<std::string> prune(State& s, bool restarting);

/* Every workspace's data folder rant started a node from, kept in ~/.rant/workspaces so a
 * stop anywhere on the machine finds them. A folder with no state left drops out. */
void remember(const fs::path& data_dir);
std::vector<fs::path> remembered();

}
