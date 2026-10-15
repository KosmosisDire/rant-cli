#pragma once

#include <filesystem>
#include <optional>
#include <string>

#include "app/context.hpp"
#include "net/github.hpp"

/* The VS Code extension, installed through the editor's own command line from a release of
 * rant-vscode. Shared by the vscode command, update, which keeps it current, and uninstall.
 * Those two act only when rant installed it, which a marker in ~/.rant records. */
namespace commands::vscode {

/* The extension's version in what `code --list-extensions --show-versions` prints, empty
 * when it is not there. */
std::string version_in(const std::string& listing);

/* The editor rant installed the extension into, nullopt when it installed none or that
 * editor is gone. */
std::optional<std::filesystem::path> recorded_editor();

/* The extension's version in an editor, empty when the editor does not have it. */
std::string installed_version(const std::filesystem::path& editor);

/* The newest extension release. Throws app::Failure. */
net::Release latest();

/* Downloads the release's extension and installs it into the editor. Throws app::Failure. */
void install(const app::Context& ctx, const std::filesystem::path& editor, const net::Release& r);

/* Removes the extension from the editor and forgets it. Throws app::Failure. */
void remove(const std::filesystem::path& editor);

}
