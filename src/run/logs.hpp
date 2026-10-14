#pragma once

#include <string>
#include <vector>

#include "config/config.hpp"

/* The logs nodes write: <logs>/<name>.log, the run before kept as <name>.log.1. */
namespace run {

/* The last n lines of a log, fewer when it is shorter, none when it cannot be read. */
std::vector<std::string> tail(const config::fs::path& log, size_t n);

}
