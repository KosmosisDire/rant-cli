#pragma once

#include <filesystem>

/* The service process, `rant __services`, one per rant home. It starts every node whose
 * restart is set as its own child, so it learns how the node ended, and starts it again as
 * its restart says. rant run starts it when such a node starts, and it ends once it has
 * nothing left to watch. */
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

/* The service process itself. */
int serve();

}
