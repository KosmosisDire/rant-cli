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
    std::string extension;       /* the VS Code extension it installed, empty for none */
    fs::path    editor;          /* and the editor and profile that took it */
    std::string profile;
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

/* What a package's package.rant says of it, at its newest release or as its source has it now. */
struct About {
    std::string version;        /* the newest release, empty when not asked or from a path */
    std::string description;
    std::string archive;        /* the files that release carries, empty for a kind it lacks */
    std::string vsix;
};

/* Reads a source's package.rant: at its newest release when release, else at the repo's
 * default branch, which costs no GitHub API call. Throws app::Failure. */
About about(const Source& s, bool release);

/* A package rant install can install: where it comes from, the description its package.rant
 * gives at the default branch, and its record when installed. problem says why there is no
 * description. */
struct Offer {
    Source                source;
    std::string           description;
    std::optional<Record> record;
    std::string           problem;
};

/* The packages the sources name whose name matches pattern, as rant ls matches, each from the
 * first source that names it. Throws app::Failure. */
std::vector<Offer> offers(Sources& sources, const std::string& pattern = "");

/* The names the databases listed when last fetched, from the cache, for completion. */
std::vector<std::string> cached_names();

/* The installed packages by name: folders in dir() and records. */
std::vector<std::string> installed();

/* The record of an installed package, nullopt when rant install left none. */
std::optional<Record> record(const std::string& name);

/* What updating an installed package would do. */
struct Update {
    Source      source;
    std::string had;             /* the version installed, empty from a path */
    std::string to;              /* the release it would move to, empty from a path */
    bool        needed = false;
};

/* What its source offers an installed package now. nullopt for one update leaves alone, one
 * installed with --from or by hand, after a note saying so. Throws app::Failure. */
std::optional<Update> check(app::Context& ctx, Sources& sources, const std::string& name);

/* A node an installed package declares, as rant's home sees it, nullopt when there is none. */
std::optional<config::NodeType> installed_node(const std::string& package, const std::string& node);

/* Installs a package from its source, the release at version or the newest, over the one
 * installed. A VS Code extension goes into the editor --editor and --profile pick, else the
 * one it went into before. Throws app::Failure. */
void install(app::Context& ctx, const Source& s, const std::string& version);

/* Removes an installed package, and its extension from the editor that took it. Throws
 * app::Failure, also while one of its nodes runs. */
void remove(app::Context& ctx, const std::string& name);

/* Drops each package that links a folder, so removing rant's home never reaches into it. */
void unlink_all();

/* Whether p is a symbolic link or, on Windows, a junction. */
bool is_link(const fs::path& p);

}
