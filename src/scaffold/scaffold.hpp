#pragma once

#include <filesystem>
#include <string>
#include <vector>

#include "config/config.hpp"

/* The templates of rant new: a folder of files rendered with inja, file names too, beside a
 * template.hcl the config crate reads. The built in ones live in templates/ and are compiled
 * in. A name's cases are functions: {{ snake(name) }}, kebab, pascal and camel. */
namespace scaffold {

namespace fs = std::filesystem;

/* A template: compiled in by name, else the folder dir. */
struct Origin {
    std::string builtin;
    fs::path    dir;
};

/* A template's description and params, for help. Diagnostics say what is wrong with it.
 * Throws app::Failure when there is no such template. */
config::TemplateManifest describe(const Origin& origin);

/* What a template made: every file written, and the next steps it suggests, maybe empty. */
struct Made {
    std::vector<fs::path> files;
    std::string           next;
};

/* Makes name from a template in dest with "key=value" params. Writes nothing when a file
 * it would write exists. Throws app::Failure. */
Made make(const Origin& origin, const fs::path& dest, const std::string& name, const std::vector<std::string>& params);

/* A name in one case: "snake", "kebab", "pascal" or "camel". Its words split at anything
 * not a letter or digit and where a lower case letter meets an upper case one. */
std::string in_case(const std::string& name, const std::string& which);

}
