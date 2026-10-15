#pragma once

#include "app/context.hpp"
#include "config/config.hpp"

/* What `rant build` does, shared with restart, which builds what it restarts first. */
namespace commands {

/* Runs a build's steps in order, configuring CMake first after a question. Returns how
 * many packages it built. Throws app::Failure at the first step that fails. */
int build_steps(app::Context& ctx, const config::Build& b);

}
