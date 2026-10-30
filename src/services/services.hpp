#pragma once

#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "process/supervisor.hpp"

/* The service process, `rant __services`, one per rant home. It starts every node whose
 * restart is set as its own child, so it learns how the node ended, and starts it again as
 * its restart says. rant run starts it when such a node starts, the boot and login entries
 * to start the services of each workspace rant enable enabled, and it ends once it has
 * nothing left to watch. */
namespace services {

namespace fs = std::filesystem;

/* Where it writes what it starts and why. */
fs::path log_file();

/* This process's Windows session, 0 elsewhere. Each session has its own service process,
 * since a process gets Ctrl-Break and window messages only from its own session. */
uint32_t session();

/* Whether the service process of this session, or of the session given, runs now. */
bool running();
bool running_in(uint32_t session);

/* Starts the one of this session unless it runs, and waits until it does. Throws app::Failure. */
void ensure_running();

/* Asks the one of a session to look for nodes to start now rather than at its next round. */
void wake(uint32_t session);

/* Asks the one of another session to stop a node there, and waits until it says how. nullopt
 * when none runs there or it does not answer, and then the caller stops the node itself. */
std::optional<process::Stopped> stop_through(uint32_t session, const process::Tracking& t);

/* The service process itself. start: first start what is enabled to start then, "boot" or
 * "login", or nothing when empty. */
int serve(const std::string& start);

/* A workspace rant enable enabled, or the rant home outside one, whose services the service
 * process starts at every boot or login with a bare rant run in its folder. */
struct Enabled {
    fs::path                           folder;
    std::string                        start = "boot";    /* or "login" */
    std::optional<uint16_t>            domain;    /* --domain as given, which every node then takes */
    std::map<std::string, std::string> env;       /* the placement variables rant enable saw */
};

/* What is enabled, in the order it was. Throws app::Failure when the list is damaged. */
std::vector<Enabled> enabled();

/* Writes the list. Throws app::Failure. */
void save(const std::vector<Enabled>& list);

}
