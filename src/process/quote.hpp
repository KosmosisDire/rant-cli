#pragma once

#include <string>
#include <vector>

/* Windows command lines. Windows passes one string, and each program splits it again: an
 * executable through the MSVC rules, a batch file through cmd.exe, which also expands
 * %VARS% and reads & and |. These build the string each side splits back correctly.
 * Plain string work, so they build and test on every OS. */
namespace process {

/* argv as one command line for CreateProcess, split back by the MSVC C runtime rules. */
std::string windows_command_line(const std::vector<std::string>& argv);

/* The cmd.exe command line that runs a batch script with args. Every argument reaches the
 * script as given, never expanded or run, the fix for CVE-2024-24576. Throws
 * std::invalid_argument for an argument cmd.exe cannot carry safely (a CR or LF) or a script
 * name holding a quote. */
std::string batch_command_line(const std::string& script, const std::vector<std::string>& args);

}
