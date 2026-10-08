#pragma once

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>

#include "process/command.hpp"

/* Detached processes the CLI starts, finds again in a later run, and stops. A process is
 * tracked with every descendant, so a wrapper such as `uv run` or `npm run` stops whole.
 * POSIX tracks a process group, Windows a named job object. */
namespace process {

/* What the state file keeps to find a started process again. */
struct Tracking {
    uint64_t    pid = 0;          /* the spawned process, the process group id on POSIX */
    uint64_t    start_time = 0;   /* its start time, which tells it from a reused pid */
    std::string job;              /* the Windows job object name, empty on POSIX */
};

/* Starts c in its own session or process group with stdin from the null device and stdout
 * and stderr appended to log. job names the Windows job object and is ignored elsewhere.
 * Throws app::Failure when it cannot start. */
Tracking start_detached(const Command& c, const fs::path& log, const std::string& job);

/* This executable's own path. */
fs::path self_path();

/* Some process of the tracked group or job still runs. */
bool alive(const Tracking& t);

/* pid belongs to the tracked group or job, such as a node a wrapper started. */
bool owns(const Tracking& t, uint64_t pid);

enum class Stopped { Gracefully, Killed, AlreadyGone };

/* Asks the whole group or job to stop (SIGTERM, or Ctrl-Break on Windows), waits up to
 * grace, then kills what is left. */
Stopped stop(const Tracking& t, std::chrono::milliseconds grace);

/* Every main() that stops processes calls this first. On Windows, stop() runs this same
 * executable as a helper that joins the node's console to send Ctrl-Break, so the caller
 * never leaves its own console. Returns the helper's exit code when argv asks for the
 * helper, nullopt otherwise. */
std::optional<int> helper_main(int argc, char** argv);

}
