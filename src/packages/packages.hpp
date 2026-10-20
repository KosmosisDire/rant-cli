#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "config/config.hpp"

/* The packages rant install puts in ~/.rant/packages, a folder each that every workspace sees.
 * A package comes from a GitHub repo's releases or from a path on this machine, as
 * sources.rant, the official database and the databases sources.rant adds name it, in that
 * order. Beside each folder a record says where it came from. */
namespace packages {

namespace fs = std::filesystem;

/* The official package database, read after sources.rant. */
extern const char* const official;

/* Where one package comes from, and what named it. */
struct Source {
    std::string name;
    std::string repo;        /* owner/name, empty for a path */
    fs::path    path;        /* a local build folder or a .tar.gz archive */
    std::string named_by;    /* "sources.rant", a database's repo, or "--from" */
};

/* What rant install recorded of a package it installed. */
struct Record {
    std::string name;
    std::string repo;
    fs::path    path;
    bool        once = false;    /* installed with --from, which update leaves alone */
    std::string version;         /* the release, empty from a path */
};

/* ~/.rant/packages. */
fs::path dir();

/* ~/.rant/sources.rant, which the user writes. */
fs::path sources_file();

/* The package sources, read from sources.rant once and the databases each fetched once,
 * only when a name is not found before them. */
class Sources {
public:
    /* Throws app::Failure when sources.rant is broken. */
    Sources();

    /* Where a name comes from, nullopt when no source names it. Throws app::Failure. */
    std::optional<Source> find(const std::string& name);

    /* Every package the sources name, each from the first that names it. Throws app::Failure. */
    std::vector<Source> all();

    /* What sources.rant says, empty without one. */
    const config::Database& local() const { return local_; }

    /* The databases in the order they are read, the official one first. */
    std::vector<std::string> databases() const;

private:
    const config::Database& fetched(size_t i);

    config::Database                                                    local_;
    std::vector<std::pair<std::string, std::optional<config::Database>>> databases_;
};

/* The names the databases listed when last fetched, from the cache, for completion. */
std::vector<std::string> cached_names();

/* The installed packages by name: folders in dir() and records. */
std::vector<std::string> installed();

/* The record of an installed package, nullopt when rant install left none. */
std::optional<Record> record(const std::string& name);

/* Installs a package from its source, the release at version or the newest, over the one
 * installed. Throws app::Failure. */
void install(app::Context& ctx, const Source& s, const std::string& version);

/* Removes an installed package. Throws app::Failure, also while one of its nodes runs. */
void remove(app::Context& ctx, const std::string& name);

/* Drops each package that links a folder, so removing rant's home never reaches into it. */
void unlink_all();

/* Whether p is a symbolic link or, on Windows, a junction. */
bool is_link(const fs::path& p);

}
