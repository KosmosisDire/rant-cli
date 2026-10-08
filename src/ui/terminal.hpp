#pragma once

#include <cstdio>
#include <string>
#include <vector>

namespace ui {

bool is_terminal(std::FILE* f);

/* Turns on ANSI escapes and UTF-8 output on a Windows console, a no op elsewhere. Returns
 * whether f is a terminal that takes escapes, for colors and for redrawing in place. */
bool enable_vt(std::FILE* f);

/* The visible rows and columns of the terminal behind stdout, 0 and 0 when unknown. */
struct Size {
    int rows = 0;
    int cols = 0;
};
Size terminal_size();

/* The command line as UTF-8 on every OS. On Windows argv is in the ANSI code page. */
std::vector<std::string> utf8_args(int argc, char** argv);

}
