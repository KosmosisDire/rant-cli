#pragma once

#include <filesystem>
#include <string>

#include "app/context.hpp"
#include "net/github.hpp"

/* The explorer rant installs into ~/.rant/bin, shared by explore, which installs it when
 * missing, and update, which keeps it current. */
namespace commands::explorer {

/* Where rant installs the explorer, whether or not it is there. */
std::filesystem::path installed();

/* The version rant installed, empty when it installed none. */
std::string installed_version();

/* The newest explorer release. Throws app::Failure. */
net::Release latest();

/* Downloads the release's explorer for this platform over the installed one. */
void install(const app::Context& ctx, const net::Release& r);

}
