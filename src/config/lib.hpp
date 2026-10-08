#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

/* The C++ face of the crate's library and release calls: where packages take Rant from,
 * the edits that move it, and GitHub releases and downloads. Values are copied out, so
 * nothing here points into Rust memory. */
namespace config {

namespace fs = std::filesystem;

inline constexpr const char* rant_repo = "KosmosisDire/Rant";

enum class LibKind { CMake, Python, CSharp };
const char* lib_kind_name(LibKind k);    /* "cmake", "python", "csharp" */

/* One place a package takes Rant from. */
struct LibUse {
    LibKind                    kind = LibKind::CMake;
    std::string                how;        /* "CPM", "FetchContent", "venv", "PackageReference", ... */
    fs::path                   dir;        /* the package folder */
    fs::path                   file;       /* the build file, or the venv folder */
    std::optional<std::string> version;    /* none when not named, or not in the venv */
    bool                       venv_exists = false;
    fs::path                   venv_python;
    std::vector<std::string>   python;     /* the interpreter that makes a missing venv */
};

/* The uses in one folder, where any Python file counts, or with recursive in every folder
 * under it, where only Python that imports rant does. */
std::vector<LibUse> lib_uses(const fs::path& path, bool recursive);

/* What an edit or a download came to. note may carry a line for the user. */
struct Outcome {
    std::string error;    /* empty on success */
    std::string note;
};

/* Moves the version a CMakeLists, pyproject or C# project names. */
Outcome lib_set(const fs::path& file, const std::string& version);

/* Adds Rant to a CMakeLists or a C# project. The note is a line still to add by hand. */
Outcome lib_add(LibKind kind, const fs::path& file, const std::string& version);

struct Asset {
    std::string name;
    std::string url;
};

struct Release {
    std::string        tag;
    std::string        version;    /* the tag without its v */
    std::vector<Asset> assets;
    std::string        error;      /* set when the release could not be found */

    const Asset* asset(const std::string& name) const;
};

/* A release of owner/name, the latest when tag is empty. */
Release release(const std::string& repo, const std::string& tag = "");

/* Downloads url to dest, which is whole or untouched afterwards. */
Outcome download(const std::string& url, const fs::path& dest);

}
