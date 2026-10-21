#pragma once

#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

/* The service process, `rant __services`, one per rant home. It starts every node whose
 * restart is set as its own child, so it learns how the node ended, and starts it again as
 * its restart says. rant run starts it when such a node starts, the boot and login entries
 * to start what rant enable named, and it ends once it has nothing left to watch. */
namespace services {

namespace fs = std::filesystem;

/* Where it writes what it starts and why. */
fs::path log_file();

/* Whether it runs now. */
bool running();

/* Starts it unless it runs, and waits until it does. Throws app::Failure. */
void ensure_running();

/* Asks it to look for nodes to start now rather than at its next round. */
void wake();

/* The service process itself. start: first start what is enabled to start then, "boot" or
 * "login", or nothing when empty. */
int serve(const std::string& start);

/* A group or a node rant enable named, which the service process starts at every boot or
 * login with rant run in its folder. */
struct Enabled {
    fs::path                           folder;    /* the workspace, or the rant home outside one */
    std::string                        start = "boot";    /* or "login" */
    std::string                        kind;      /* "group" or "node" */
    std::string                        name;      /* the group or the node type, in full */
    std::vector<std::string>           words;     /* a group's key=value words as given */
    std::optional<uint16_t>            domain;    /* --domain as given, which every node then takes */
    std::map<std::string, std::string> env;       /* the placement variables rant enable saw */

    /* "Nav speed=2", or "node demo/talker". */
    std::string shown() const;

    /* The same folder, group or node and words, whenever it starts. */
    bool same(const Enabled& o) const;
};

/* What is enabled, in the order it was. Throws app::Failure when the list is damaged. */
std::vector<Enabled> enabled();

/* Writes the list. Throws app::Failure. */
void save(const std::vector<Enabled>& list);

}
