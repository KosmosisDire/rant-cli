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
};

/* Where a program runs from. A name holding a slash is taken relative to cwd, a bare name
 * is looked up on PATH, through PATHEXT on Windows. The current directory is never searched
 * for a bare name. nullopt when nothing is found. */
std::optional<fs::path> find_program(const std::string& program, const fs::path& cwd);

/* Runs a command to its end with this process's stdin, stdout and stderr, and returns its
 * exit code. Throws app::Failure when it cannot start. */
int run(const Command& c);

/* The inherited environment with env applied, as sorted "NAME=value" entries. Names match
 * without case on Windows. */
std::vector<std::string> environment(const std::map<std::string, std::string>& env);

/* The words of a command for messages, quoted where a word holds a space. */
std::string shown(const std::vector<std::string>& argv);

}
