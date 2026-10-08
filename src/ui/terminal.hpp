#pragma once

#include <cstdio>
#include <string>
#include <vector>

namespace ui {

bool is_terminal(std::FILE* f);

/* Turns on ANSI colors and UTF-8 output on a Windows console, a no op elsewhere. Returns
 * whether colors can be shown on f. */
bool enable_colors(std::FILE* f);

/* The command line as UTF-8 on every OS. On Windows argv is in the ANSI code page. */
std::vector<std::string> utf8_args(int argc, char** argv);

}
