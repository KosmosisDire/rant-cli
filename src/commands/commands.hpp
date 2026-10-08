#pragma once

#include <vector>

#include "app/command.hpp"

/* Every subcommand, one file each. The registry in commands.cpp lists them in help order. */
namespace commands {

const std::vector<app::Command>& all();

app::Command init();
app::Command new_();
app::Command build();
app::Command start();
app::Command stop();
app::Command restart();
app::Command ls();
app::Command info();
app::Command sub();
app::Command pub();
app::Command get();
app::Command set();
app::Command call();
app::Command setup();
app::Command lib();
app::Command explore();

}
