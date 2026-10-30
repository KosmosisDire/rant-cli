#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "config/config.hpp"

/* Where packages take each library from and at which version, and the edits that add or move
 * it. Build files are matched by pattern, never parsed, so hand edits work within reason. */
namespace library {

namespace fs = std::filesystem;

/* A library rant add can add: its name, the GitHub repo whose releases carry it, and the names
 * each language takes it by, as its package.rant gives them. */
struct Library {
    std::string          name;
    std::string          repo;
    config::LibraryBlock names;
};

/* The library added when none is named, the one Rant nodes are built on. */
inline constexpr const char* default_library = "rant";

enum class Kind { CMake, Python, CSharp, Rust };
const char* kind_name(Kind k);    /* "cmake", "python", "csharp", "rust" */

/* Whether the library names a way for that kind of package to take it. */
bool supports(const Library& lib, Kind k);

/* The library's Python project as its wheels and their dist-info folders spell it: lower case,
 * each run of -, _ and . as one _. */
std::string dist_name(const Library& lib);

/* One place a package takes a library from. how says which, and so whether rant can move it:
 * "CPM", "FetchContent", "find_package", "pyproject", "venv", "PackageReference",
 * "ProjectReference", "Cargo", "Cargo workspace" or "Cargo path". */
struct Use {
    std::string                library;    /* its name */
    Kind                       kind = Kind::CMake;
    std::string                how;
    fs::path                   dir;        /* the package folder */
    fs::path                   file;       /* the build file, or the venv, which may not exist yet */
    std::optional<std::string> version;    /* none when not named, or not in the venv */
    bool                       venv_exists = false;
    bool                       wanted = true;    /* false for a use whose version rant leaves to another file */
    fs::path                   venv_python;
    std::vector<std::string>   python;     /* the interpreter that makes a missing venv */
};

/* The uses of each library in one folder. adding counts any Python file, so a library can be
 * added to it, else Python counts that lists it, holds it, or imports rant for the default. */
std::vector<Use> in_folder(const fs::path& dir, const std::vector<Library>& libs, bool adding = false);

/* The uses in every folder under start, by folder, the walk following the workspace's ignore
 * globs. A venv holding a library is listed once, under the folder that holds it. */
std::vector<Use> under(const fs::path& start, const std::vector<Library>& libs);

/* Moves the version of lib a CMakeLists, pyproject, C# project or Cargo.toml names. Throws app::Failure. */
void set_version(const fs::path& file, const Library& lib, const std::string& version);

/* Adds lib at version, its release tagged tag, to a CMakeLists, a C# project or a Cargo.toml.
 * Returns a line still to add by hand, empty when there is none. Throws app::Failure. */
std::string add(Kind kind, const fs::path& file, const Library& lib, const std::string& version, const std::string& tag);

/* Adds a node to a CMakeLists: its own executable built from source, a path from the
 * CMakeLists folder, linked to Rant. Throws app::Failure when the target exists. */
void add_node(const fs::path& cmakelists, const std::string& name, const std::string& source);

/* The file a node is written in: a Python node's own file, the first source of the CMake
 * target named after it, the C# file holding its Main, a Rust program's source. */
std::optional<fs::path> node_source(const config::NodeType& n);

/* The first source a CMakeLists gives target, as written: from add_executable, else
 * target_sources. Variables and generator expressions are passed over. */
std::optional<std::string> cmake_source(const std::string& text, const std::string& target);
/* Whether C# text declares a Main entry point. */
bool csharp_main(const std::string& text);

/* The pure text edits behind set_version, add and add_node, for the tests. Throw app::Failure. */
std::string cmake_set(const std::string& text, const Library& lib, const std::string& version);
std::string cmake_add(const std::string& text, const Library& lib, const std::string& tag, std::string* hint);
std::string cmake_add_node(const std::string& text, const std::string& name, const std::string& source);
std::string pyproject_set(const std::string& text, const Library& lib, const std::string& version);
std::string csharp_set(const std::string& text, const Library& lib, const std::string& version);
std::string csharp_add(const std::string& text, const Library& lib, const std::string& version);
std::string cargo_set(const std::string& text, const Library& lib, const std::string& version);
std::string cargo_add(const std::string& text, const Library& lib, const std::string& version);
std::optional<Use> cmake_use(const std::string& text, const Library& lib);
std::optional<Use> csharp_use(const std::string& text, const Library& lib);
std::optional<Use> cargo_use(const std::string& text, const Library& lib);

}
