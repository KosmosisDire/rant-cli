#pragma once

#include <filesystem>
#include <string>
#include <vector>

/* The login entry that starts the service process for a rant home: a Run value on Windows, a
 * systemd user unit on Linux, a launchd agent on macOS. It names the rant and the home that
 * wrote it, and a rant only ever changes the entry naming itself and its home. */
namespace services::entry {

namespace fs = std::filesystem;

/* What the entry runs: `rant __services --login HOME`. */
std::vector<std::string> argv(const fs::path& rant, const fs::path& home);

/* The systemd user unit and the launchd agent, as text. */
std::string systemd_unit(const std::vector<std::string>& argv);
std::string launchd_plist(const std::vector<std::string>& argv, const fs::path& log);

/* The entry exists and names this rant and this home. */
bool present();

/* Writes the entry for this rant and home, over one another wrote. Throws app::Failure. */
void write();

/* Removes the entry when it names this rant and home. */
void remove();

}
