#pragma once

#include <filesystem>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "config/config.hpp"

/* The vsix kind: a VS Code extension a package's release carries, installed through the
 * editor's own command line, into the profile in use. */
namespace packages::vsix {

/* A profile from the editor's storage.json. id is empty for the default profile, and shared
 * is true for one that uses the default profile's extensions. */
struct Profile {
    std::string id;
    std::string name;
    bool        shared = false;
};

/* Where an extension goes: the editor's command line program and a profile name in it,
 * empty for the default profile. */
struct Target {
    std::filesystem::path editor;
    std::string           profile;
};

/* A target for messages: the editor, then the profile past the default. */
std::string shown(const Target& t);

/* An extension's version in what `code --list-extensions --show-versions` prints, empty
 * when it is not there. */
std::string version_in(const std::string& listing, const std::string& id);

/* The profiles in the text of a storage.json, the default first. */
std::vector<Profile> profiles_in(const std::string& storage);

/* The ids of the profiles a storage.json gives the folders that hold dir, without repeats.
 * Several mean nested folders opened in different profiles. */
std::vector<std::string> folder_profiles(const std::string& storage, const std::filesystem::path& dir);

/* --editor, else VS Code's `code`, and --profile, else the profile this VS Code terminal's
 * folder opens in, else the default when no other profile has its own extensions, else the
 * one the user picks. Throws app::Failure. */
Target target(const app::Context& ctx);

/* Whether target would find an editor: --editor's, else VS Code's `code`. */
bool editor_found(const app::Context& ctx);

/* An extension's version in a target, empty when it does not have it. */
std::string installed_version(const Target& t, const std::string& id);

/* Installs a .vsix through the target's editor. Returns its id and version. Throws app::Failure. */
config::Vsix install(const Target& t, const std::filesystem::path& file);

/* Removes an extension from the target. Throws app::Failure. */
void remove(const Target& t, const std::string& id);

}
