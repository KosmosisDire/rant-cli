#pragma once

#include "app/args.hpp"
#include "app/context.hpp"
#include "config/config.hpp"

/* What `rant build` does, shared with run and restart, which build what they start first. */
namespace commands {

/* Runs a build's steps in order, configuring CMake first after a question. clean removes
 * every package's build output first. Returns how many packages it built. Throws
 * app::Failure at the first step that fails. */
int build_steps(app::Context& ctx, const config::Build& b, bool clean);

/* Builds the packages whose nodes the plans run, and what they depend on, so run and restart
 * start the code as it is now. Build tools only redo what changed. --no-build skips it. A
 * failed build is reported and false, and the caller starts what is on disk, the last build
 * that worked. */
bool build_for(app::Context& ctx, const std::vector<config::Plan>& plans);

/* The --no-build option of run and restart. */
app::OptionSpec no_build_option();

}
