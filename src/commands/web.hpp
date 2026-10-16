#pragma once

#include <string>
#include <vector>

#include "app/context.hpp"
#include "commands/service.hpp"

/* The web explorer: the installed explorer run with --web-server in the background, so any
 * browser on the network opens the explorer as a page. Shared by `rant explore web`, which
 * sets it up, update, which restarts it on a new explorer, and uninstall. */
namespace commands::web {

/* The web explorer as a service of this home, at the address `rant explore web enable` saved. */
service::Service service();

/* The options only enable takes. */
const std::vector<std::string>& options();

/* `rant explore web`, words from the subcommand on. */
int run(app::Context& ctx, const std::vector<std::string>& words);

}
