#pragma once

#include <filesystem>
#include <string>
#include <utility>
#include <vector>

/* The presets of rant new: folders of files under templates/, compiled in. {{name}} in a
 * file's path or text is the name given, nothing else is filled in. */
namespace scaffold {

namespace fs = std::filesystem;

/* Files to write, each with its text. */
using Files = std::vector<std::pair<fs::path, std::string>>;

/* The files a preset makes for name in dest. Throws app::Failure when one exists already. */
Files plan(const std::string& preset, const fs::path& dest, const std::string& name);

/* Writes files, making their folders. Throws app::Failure. */
void write(const Files& files);

}
