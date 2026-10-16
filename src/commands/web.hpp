#pragma once

#include "commands/service.hpp"

/* The web explorer: the installed explorer run with --web-server in the background, so any
 * browser on the network opens the explorer as a page. Shared by the web command, which sets
 * it up, update, which restarts it on a new explorer, and uninstall. */
namespace commands::web {

/* The web explorer as a service of this home, at the address rant web enable saved. */
service::Service service();

}
