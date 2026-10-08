#pragma once

#include <string>
#include <string_view>
#include <vector>

#include "app/args.hpp"
#include "app/context.hpp"

namespace complete {
struct Candidates;
class Request;
}

namespace app {

enum class Section { Workspace, Mesh, Setup };

/* One subcommand. own_help makes --help reach run, for help that depends on the words,
 * such as a group's params. complete offers the words at the cursor for tab completion. */
struct Command {
    std::string_view        name;
    std::string_view        usage;      /* the words after the name, "<topic>" */
    std::string_view        summary;
    Section                 section;
    std::vector<OptionSpec> options;
    int                   (*run)(Context&);
    bool                    own_help = false;
    complete::Candidates  (*complete)(complete::Request&) = nullptr;
};

const std::vector<OptionSpec>& global_options();

/* Parses the command line, runs the command and returns the exit code. Throws Failure. */
int run(Context& ctx, const std::vector<std::string>& tokens);

/* The usage block of one command, for its --help and for usage errors. */
std::string command_help(const Command& c);

}
