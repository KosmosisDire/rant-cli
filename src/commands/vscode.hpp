#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "net/github.hpp"

/* The VS Code extension, installed through the editor's own command line from a release of
 * rant-vscode. Shared by the vscode command, update, which keeps it current, and uninstall.
 * Those two act only when rant installed it, which a marker in ~/.rant records. */
namespace commands::vscode {

/* A profile from the editor's storage.json. id is empty for the default profile, and shared
 * is true for one that uses the default profile's extensions. */
struct Profile {
    std::string id;
    std::string name;
    bool        shared = false;
};

/* Where the extension goes: the editor's command line program and a profile name in it,
 * empty for the default profile. */
struct Target {
    std::filesystem::path editor;
    std::string           profile;
};

/* A target for messages: the editor, then the profile past the default. */
std::string shown(const Target& t);

/* The extension's version in what `code --list-extensions --show-versions` prints, empty
 * when it is not there. */
std::string version_in(const std::string& listing);

/* The profiles in the text of a storage.json, the default first. */
std::vector<Profile> profiles_in(const std::string& storage);

/* The ids of the profiles a storage.json gives the folders that hold dir, without repeats.
 * Several mean nested folders opened in different profiles. */
std::vector<std::string> folder_profiles(const std::string& storage, const std::filesystem::path& dir);

/* Where rant installed the extension, nullopt when it installed none or that editor is gone. */
std::optional<Target> recorded();

/* The extension's version in a target, empty when it does not have it. */
std::string installed_version(const Target& t);

/* The newest extension release. Throws app::Failure. */
net::Release latest();

/* Downloads the release's extension and installs it into the target. Throws app::Failure. */
void install(const app::Context& ctx, const Target& t, const net::Release& r);

/* Removes the extension from the target and forgets it. Throws app::Failure. */
void remove(const Target& t);

}
