#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"

/* A program rant installs into ~/.rant/bin and keeps running in the background, started at
 * login by the Run key on Windows, a systemd user unit on Linux and a launchd agent on macOS.
 * The program writes a state file while it listens, which is how rant finds it again. The
 * bridge and the web explorer are two, each with a command over the same subcommands. */
namespace commands::service {

struct Service {
    std::string              name;            /* "the bridge", as messages name it */
    std::string              command;         /* "bridge", the rant command that manages it */
    std::string              description;     /* the unit's Description, "Rant bridge" */
    std::filesystem::path    exe;             /* where rant installs the program */
    std::filesystem::path    version_file;    /* the version rant installed */
    std::vector<std::string> args;            /* its arguments under systemd and launchd */
    std::vector<std::string> windows_args;    /* its arguments under the Run key, which keeps no log */
    std::filesystem::path    state;           /* what the program writes while it listens */
    std::filesystem::path    log;             /* where it logs, but on Linux, which has the journal */
    std::string              unit;            /* "rant-bridge", the systemd unit */
    std::string              label;           /* "dev.rantlib.bridge", the launchd agent */
    std::string              run_name;        /* "RantBridge", the Run value */
    std::string              stop_event;      /* the Windows event the program stops on */
    std::string              stays;           /* what disable leaves, "its binary stays" */
};

/* What a program writes in its state file once it listens. */
struct State {
    uint64_t    pid = 0;
    std::string url;
    std::string version;
    long long   started = 0;    /* unix seconds */
};

/* The state file's lines. nullopt when they name no pid. */
std::optional<State> parse_state(const std::string& text);

/* The state of the program running now, nullopt when there is no state file or its pid is gone. */
std::optional<State> running(const Service& s);

/* The version rant installed, empty when it installed none. */
std::string installed_version(const Service& s);

/* The service definition of this platform names this program. */
bool enabled(const Service& s);

/* Writes this platform's service definition, so the program starts when the user logs in.
 * Throws app::Failure. */
void define(const Service& s);

/* Starts the program, through the service manager where there is one. The caller has checked
 * that it is installed and not running. Throws app::Failure, also when a platform with a
 * service manager has it not enabled. */
void start(const Service& s);

/* Stops the running program and waits for it to end. False when none runs. Throws
 * app::Failure when it will not end. */
bool stop(const Service& s);

/* Stops and starts it again. Throws app::Failure as start does. */
void restart(const Service& s);

/* Stops the program and removes the service definition, whichever exist. */
void disable(const Service& s);

/* The state once the program listens under another process id than not_pid, or a failure.
 * A restart passes the old id so the state it replaces is not taken. */
State wait_listening(const Service& s, uint64_t not_pid);

/* What `exe --version` prints after the program name. Throws app::Failure when it does not run. */
std::string version_of(const std::filesystem::path& exe);

/* Copies a program the user built, given as --from, over the installed one, which stops
 * first, and records its version. Throws app::Failure. */
void install_from(app::Context& ctx, const Service& s, const std::string& from);

/* Throws app::Failure naming the enable subcommand when the program is not installed. */
void require_installed(const Service& s);

/* The command line the Windows Run key and a Windows start use. */
std::vector<std::string> windows_argv(const Service& s);

/* The systemd user unit and the launchd agent, as text. */
std::string systemd_unit(const Service& s);
std::string launchd_plist(const Service& s);

/* The subcommands, in the order help lists them. */
const std::vector<std::string>& subcommands();

/* Refuses words and options a subcommand does not take. enable_options are those only
 * enable takes. Throws app::UsageError. */
void check_usage(const app::Context& ctx, const Service& s, const std::vector<std::string>& enable_options);

/* Runs disable, start, stop, restart, logs or run, which every service has alike. nullopt
 * for another name. */
std::optional<int> run_shared(app::Context& ctx, const Service& s, const std::string& sub);

}
