#pragma once

#include "app/context.hpp"
#include "config/config.hpp"

namespace commands {

/* Throws app::Failure after printing every diagnostic of a plan that did not resolve. */
void require_plan(app::Context& ctx, const config::Plan& plan);

/* With --domain every node of the plan goes on that domain, whatever its placement says. */
void place_on_domain(app::Context& ctx, config::Plan& plan);

/* The params a group takes, as --help lists them. Nothing when there are none. */
void print_params(app::Context& ctx, const std::vector<config::Param>& params);

/* A note for each name against the convention: a group such as nav for Nav, a node
 * such as ArmController for arm_controller. A name may carry a package or prefix before /. */
void note_names(app::Context& ctx, const std::vector<std::string>& groups, const std::vector<std::string>& nodes);

/* A resolved plan for --dry-run: each instance with exactly how it would run, or with
 * --json only absolute paths, so no other tool needs to resolve anything again. */
void print_plan(app::Context& ctx, const config::Plan& plan);

}
