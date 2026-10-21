#pragma once

#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

/* Running programs from an argv, never through a shell. Building and spawning nodes both
 * go through here. */
namespace process {

namespace fs = std::filesystem;

struct Command {
    std::vector<std::string>           argv;    /* argv[0] is the program */
    fs::path                           cwd;
    std::map<std::string, std::string> env;     /* set on top of the inherited environment */
    std::vector<std::string>           unset;   /* inherited names left out */
    std::optional<std::vector<std::string>> base;    /* "NAME=value" entries inherited in place of this process's */
};

/* Where a program runs from. A name holding a slash is taken relative to cwd, a bare name
 * is looked up on PATH, through PATHEXT on Windows. The current directory is never searched
 * for a bare name. nullopt when nothing is found. */
std::optional<fs::path> find_program(const std::string& program, const fs::path& cwd);

/* find_program for a command's argv[0], or a failure saying where it was looked for. */
fs::path require_program(const Command& c);

/* Runs a command to its end with this process's stdin, stdout and stderr, and returns its exit
 * code. With a file out, stdout goes there and stderr nowhere. Throws app::Failure when it cannot start. */
int run(const Command& c, const fs::path& out = {});

/* run as an administrator: through the admin prompt on Windows, which passes no env and
 * shows no output, else through sudo, which asks in this terminal. Throws app::Failure when
 * it cannot start or the prompt is declined. */
int run_elevated(const Command& c);

/* What a command printed on stdout, nullopt when it cannot start or exits nonzero. What it
 * says on stderr is dropped, since a caller only probes. */
std::optional<std::string> output(const Command& c);

/* The inherited environment, or the command's base, with its env and unset applied, as sorted
 * "NAME=value" entries. Names match without case on Windows. */
std::vector<std::string> environment(const Command& c);

/* The words of a command for messages, quoted where a word holds a space. */
std::string shown(const std::vector<std::string>& argv);

}
