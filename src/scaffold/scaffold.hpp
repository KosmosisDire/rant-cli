#pragma once

#include <filesystem>
#include <map>
#include <string>
#include <utility>
#include <vector>

/* The presets of rant new: folders of files under templates/, compiled in. {{key}} in a
 * file's path or text is the value given for key, nothing else is filled in. */
namespace scaffold {

namespace fs = std::filesystem;

/* Files to write, each with its text. */
using Files = std::vector<std::pair<fs::path, std::string>>;

/* What fills each {{key}}: name always, framework for a C# preset. */
using Values = std::map<std::string, std::string>;

/* The files a preset makes in dest. Throws app::Failure when one exists already. */
Files plan(const std::string& preset, const fs::path& dest, const Values& values);

/* Writes files, making their folders. Throws app::Failure. */
void write(const Files& files);

}
