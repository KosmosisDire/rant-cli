#pragma once

#include <filesystem>
#include <map>
#include <string>
#include <vector>

#include "process/supervisor.hpp"

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

    /* Same type, argv, env and working directory. */
    bool same_spec(const Instance& o) const;
};

struct State {
    std::vector<Root>     roots;
    std::vector<Instance> instances;    /* in start order */

    Instance* instance(const std::string& name);
    Root*     root(const std::string& key);
};

/* The state file, locked from construction to destruction so two commands never act on it
 * at once. A command blocked on the lock waits for the other to finish. */
class Store {
public:
    explicit Store(const fs::path& data_dir);
    ~Store();
    Store(const Store&) = delete;
    Store& operator=(const Store&) = delete;

    State& state() { return state_; }

    /* Writes a temp file and renames it into place, so a crash never leaves half a file. */
    void save();

private:
    fs::path path_;
    State    state_;
#ifdef _WIN32
    void*    lock_ = nullptr;
#else
    int      lock_ = -1;
#endif
};

/* Drops the instances whose processes are all gone, their names leave every root, and a
 * node root left with nothing goes too. Group roots stay, so starting the group again
 * brings back what crashed. Returns the names dropped. */
std::vector<std::string> prune(State& s);

}
