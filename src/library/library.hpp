#pragma once

#include <filesystem>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "library/uses.hpp"
#include "net/github.hpp"
#include "packages/packages.hpp"

/* Bringing packages to a library's release: build files edited, venvs pip installed, C# packages
 * put in a local NuGet feed. `rant add`, `rant update` and `rant new` share it. */
namespace library {

/* The library a word names: a name the sources give, or a repo's link. Throws app::Failure,
 * also for a package, which rant install takes. */
Library find(const app::Context& ctx, packages::Sources& sources, const std::string& word);

/* Every library the sources name, read now. */
std::vector<Library> all(packages::Sources& sources);

/* The libraries the sources named when last read, read now only when the package database
 * never was, so showing what packages use needs no network. */
std::vector<Library> known();

/* The release of lib asked for, the latest when version is empty. Throws app::Failure. */
net::Release release(const Library& lib, const std::string& version);

/* The local NuGet feed under ~/.rant that C# packages get libraries from, and whether dotnet's
 * user config names it. */
std::filesystem::path nuget_feed();
bool nuget_feed_added();

/* Adds the release to dir's build files, or moves them to it, never back unless asked, and offers
 * its skill. False when any file failed. Throws app::Failure when dir has no file lib takes. */
bool add(const app::Context& ctx, const std::filesystem::path& dir, const Library& lib, const net::Release& r, bool asked);

/* Brings each use and installed skill to its library's latest release, never back, once the user
 * agrees to the list. False when any failed or the answer was no. Throws app::Failure. */
bool update(const app::Context& ctx, const std::vector<Use>& uses, const std::vector<Library>& libs);

}
