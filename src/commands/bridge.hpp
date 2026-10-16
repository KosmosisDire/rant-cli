#pragma once

#include <filesystem>
#include <string>

#include "app/context.hpp"
#include "commands/service.hpp"
#include "net/github.hpp"

/* The bridge rant installs into ~/.rant/bin and keeps running in the background, shared by
 * the bridge command, which sets it up, update, which keeps it current, and uninstall. */
namespace commands::bridge {

/* Where rant installs the bridge, whether or not it is there. */
std::filesystem::path installed();

/* The newest bridge release. Throws app::Failure. */
net::Release latest();

/* Downloads the release's bridge for this platform over the installed one. */
void install(const app::Context& ctx, const net::Release& r);

/* The config the bridge reads, which rant never opens. */
std::filesystem::path config_path();

/* The bridge as a service of this home. */
service::Service service();

}
