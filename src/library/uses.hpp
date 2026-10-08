#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

/* Where packages take the Rant library from and at which version, and the edits that add
 * it or move it. Build files are matched by pattern, never fully parsed, so a file edited by
 * hand still works within reason. */
namespace library {

namespace fs = std::filesystem;

enum class Kind { CMake, Python, CSharp };
const char* kind_name(Kind k);    /* "cmake", "python", "csharp" */

/* One place a package takes Rant from. how says which, and so whether rant can move it:
 * "CPM", "FetchContent", "find_package", "pyproject", "venv", "PackageReference" or
 * "ProjectReference". */
struct Use {
    Kind                       kind = Kind::CMake;
    std::string                how;
    fs::path                   dir;        /* the package folder */
    fs::path                   file;       /* the build file, or the venv, which may not exist yet */
    std::optional<std::string> version;    /* none when not named, or not in the venv */
    bool                       venv_exists = false;
    bool                       wanted = true;    /* false for a venv found only by the walk, Rant not in it */
    fs::path                   venv_python;
    std::vector<std::string>   python;     /* the interpreter that makes a missing venv */
};

/* The uses in one folder, where any Python file counts, so Rant can be added to it. */
std::vector<Use> in_folder(const fs::path& dir);

/* The uses in every folder under start, by folder, where only Python that imports rant
 * counts, and every venv under it with the Rant it holds, if any. The walk follows the
 * workspace's ignore globs, and a venv is listed once, under the folder that holds it. */
std::vector<Use> under(const fs::path& start);

/* Moves the Rant version a CMakeLists, pyproject or C# project names. Throws app::Failure. */
void set_version(const fs::path& file, const std::string& version);

/* Adds Rant at version to a CMakeLists or a C# project. Returns a line still to add by
 * hand, empty when there is none. Throws app::Failure. */
std::string add(Kind kind, const fs::path& file, const std::string& version);

/* The pure text edits behind set_version and add, for the tests. Throw app::Failure. */
std::string cmake_set(const std::string& text, const std::string& version);
std::string cmake_add(const std::string& text, const std::string& version, std::string* hint);
std::string pyproject_set(const std::string& text, const std::string& version);
std::string csharp_set(const std::string& text, const std::string& version);
std::string csharp_add(const std::string& text, const std::string& version);
std::optional<Use> cmake_use(const std::string& text);
std::optional<Use> csharp_use(const std::string& text);

}
