#pragma once

#include <filesystem>
#include <string>

#include "app/context.hpp"
#include "config/lib.hpp"

/* Bringing packages to a Rant release: build files edited, Python venvs pip installed, the
 * C# package put in a local NuGet feed. `rant lib install` and `rant new` share it. */
namespace library {

/* The release asked for, the latest when version is empty. Throws app::Failure. */
config::Release release(const std::string& version);

/* Brings every use in dir, or under it with recursive, to the release, and adds Rant to
 * the build files of dir that lack it unless recursive. Prints a line per package and
 * returns false when any failed. Throws app::Failure when there is nothing to do. */
bool install(const app::Context& ctx, const std::filesystem::path& dir, bool recursive, const config::Release& r);

}
