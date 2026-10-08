#pragma once

#include "app/context.hpp"
#include "config/config.hpp"

namespace commands {

/* Throws app::Failure after printing every diagnostic of a plan that did not resolve. */
void require_plan(app::Context& ctx, const config::Plan& plan);

/* The params a group takes, as --help lists them. Nothing when there are none. */
void print_params(app::Context& ctx, const std::vector<config::Param>& params);

/* A resolved plan for --dry-run: each instance with exactly how it would run, or with
 * --json only absolute paths, so no other tool needs to resolve anything again. */
void print_plan(app::Context& ctx, const config::Plan& plan);

}
