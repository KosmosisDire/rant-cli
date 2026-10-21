#pragma once

#include <filesystem>
#include <string>
#include <vector>

/* The entries that start the service process for a rant home. The boot entry takes admin
 * once and stays: a scheduled task on Windows, a system unit on Linux and a launch daemon on
 * macOS, each running as the user. The login entry is a Run value, a systemd user unit or a
 * launch agent. Each names the rant and the home that wrote it, and a rant only changes an
 * entry naming itself and its home. */
namespace services::entry {

namespace fs = std::filesystem;

enum class When { Boot, Login };

/* "boot" or "login", as services.json and messages say it. */
const char* word(When when);

/* What an entry runs: `rant __services --boot HOME` or `--login HOME`. */
std::vector<std::string> argv(When when, const fs::path& rant, const fs::path& home);

/* The texts the entries are made of. user names who a boot entry runs as. */
std::string systemd_user_unit(const std::vector<std::string>& argv);
std::string systemd_system_unit(const std::vector<std::string>& argv, const std::string& user);
std::string launchd_plist(const std::vector<std::string>& argv, const std::string& label, const std::string& user, const fs::path& log);
std::string task_xml(const std::vector<std::string>& argv, const std::string& user_sid);

/* Where the boot entry goes, for messages: the task, unit or daemon. */
std::string boot_place();

/* The entry exists and names this rant and this home. */
bool present(When when);

/* Writes the entry for this rant and home, over one another wrote. The boot entry asks for
 * admin. Throws app::Failure. */
void write(When when);

/* Removes the entry when it names this rant and home. The boot entry asks for admin. */
void remove(When when);

}
