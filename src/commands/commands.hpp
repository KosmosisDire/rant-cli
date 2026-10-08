#pragma once

#include <vector>

#include "app/command.hpp"

/* Every subcommand, one file each. The registry in commands.cpp lists them in help order. */
namespace commands {

const std::vector<app::Command>& all();

app::Command init();

}
