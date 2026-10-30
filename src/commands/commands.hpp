#pragma once

#include <vector>

#include "app/command.hpp"

/* Every subcommand, one file each. The registry in commands.cpp lists them in help order. */
namespace commands {

const std::vector<app::Command>& all();

/* The command a word names, by its name or an alias. nullptr when none. */
const app::Command* find(std::string_view word);

app::Command init();
app::Command new_();
app::Command build();
app::Command run_();
app::Command stop();
app::Command restart();
app::Command enable();
app::Command disable();
app::Command log();
app::Command ls();
app::Command info();
app::Command sub();
app::Command pub();
app::Command call();
app::Command setup();
app::Command add();
app::Command install();
app::Command remove();
app::Command explore();
app::Command update();
app::Command uninstall();

}
