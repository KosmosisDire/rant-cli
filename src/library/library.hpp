#pragma once

#include <filesystem>
#include <string>

#include "app/context.hpp"
#include "net/github.hpp"

/* Bringing packages to a Rant release: build files edited, Python venvs pip installed, the
 * C# package put in a local NuGet feed. `rant lib`, `rant update` and `rant new` share it. */
namespace library {

inline constexpr const char* rant_repo = "KosmosisDire/Rant";

/* The Rant release asked for, the latest when version is empty. Throws app::Failure. */
net::Release release(const std::string& version);

/* Adds the release to the build files of dir that do not use Rant yet. Prints a line per
 * file and returns false when any failed. Throws app::Failure when dir has no build file. */
bool install(const app::Context& ctx, const std::filesystem::path& dir, const net::Release& r);

/* Brings every use under dir to the release once the user agrees to the list of changes.
 * Prints a line per package and returns false when any failed or the answer was no.
 * Throws app::Failure when nothing under dir uses Rant. */
bool update(const app::Context& ctx, const std::filesystem::path& dir, const net::Release& r);

}
