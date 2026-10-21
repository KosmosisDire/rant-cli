#pragma once

#include <string>

#include "app/context.hpp"

namespace commands {

/* `rant ls services`: what rant enable named, whether it runs and how often its nodes were
 * started again. */
int list_services(app::Context& ctx, const std::string& pattern);

}
