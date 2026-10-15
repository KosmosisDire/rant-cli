#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "net/github.hpp"

/* The bridge rant installs into ~/.rant/bin and keeps running in the background, shared by
 * the bridge command, which sets it up, update, which keeps it current, and uninstall. */
namespace commands::bridge {

/* Where rant installs the bridge, whether or not it is there. */
std::filesystem::path installed();

/* The version rant installed, empty when it installed none. */
std::string installed_version();

/* The newest bridge release. Throws app::Failure. */
net::Release latest();

/* Downloads the release's bridge for this platform over the installed one. */
void install(const app::Context& ctx, const net::Release& r);

/* The config the bridge reads, which rant never opens. */
std::filesystem::path config_path();

/* Where the bridge writes its log, when it does not use the system's. */
std::filesystem::path log_file();

/* What the bridge writes in bridge.state once it listens. */
struct State {
    uint64_t    pid = 0;
    std::string url;
    std::string version;
    long long   started = 0;    /* unix seconds */
};

/* The state file's lines. nullopt when they name no pid. */
std::optional<State> parse_state(const std::string& text);

/* The state of a bridge that runs now, nullopt when there is no state file or its pid is gone. */
std::optional<State> running();

/* The service definition of this platform names this rant's bridge. */
bool enabled();

/* Writes this platform's service definition, so the bridge starts when the user logs in.
 * Throws app::Failure. */
void define();

/* Starts the bridge, through the service manager where there is one. The caller has checked
 * that it is installed and not running. Throws app::Failure, also when a platform with a
 * service manager has the bridge not enabled. */
void start();

/* Stops a running bridge and waits for it to end. False when none runs. Throws app::Failure
 * when it will not end. */
bool stop();

/* Stops and starts it again. Throws app::Failure as start does. */
void restart();

/* Stops the bridge and removes the service definition, whichever exist. */
void disable();

/* The command line the Windows Run key and a Windows start use. */
std::vector<std::string> windows_argv(const std::string& exe, const std::string& log);

/* The systemd user unit and the launchd agent, as text. */
std::string systemd_unit(const std::string& exe, const std::string& config);
std::string launchd_plist(const std::string& exe, const std::string& config, const std::string& log);

}
