#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "config/config.hpp"
#include "net/github.hpp"

/* The packages rant install puts in ~/.rant/packages, each from a repo's releases or a path, named
 * by its package.rant as sources.rant or a package database finds it, with a record beside it. */
namespace packages {

namespace fs = std::filesystem;

/* The official package database, read after sources.rant. */
extern const char* const official;

/* Where one package comes from, and what named it. */
struct Source {
    std::string                         name;
    std::string                         repo;        /* owner/name, empty for a path */
    fs::path                            path;        /* a local build folder or a .tar.gz archive */
    std::string                         named_by;    /* "sources.rant", a database's repo, "link" or "--from" */
    std::optional<config::LibraryBlock> library;     /* set for a library, which rant add takes instead */
};

/* What rant install recorded of a package it installed. */
struct Record {
    std::string           name;
    std::string           repo;
    fs::path              path;
    std::string           named_by;     /* its source's, so update looks where install did */
    std::string           version;      /* the release, empty from a path */
    std::string           extension;    /* the VS Code extension it installed, empty for none */
    fs::path              editor;       /* and the editor and profile that took it */
    std::string           profile;
    std::vector<fs::path> skills;       /* the links that give its skill to AI coding agents */
};

/* ~/.rant/packages. */
fs::path dir();

/* ~/.rant/sources.rant, which the user writes. */
fs::path sources_file();

/* A package rant install can install: where it comes from, the description its package.rant
 * gives at the default branch or in its folder, and its record when installed. */
struct Offer {
    Source                source;
    std::string           description;
    std::optional<Record> record;
};

/* The package sources as lists read in order: the paths and repos sources.rant lists, then
 * each database. A list is read once, only when a name is not found before it, and a repo's
 * package.rant at its default branch names its package. */
class Sources {
public:
    /* Throws app::Failure when sources.rant is broken. */
    Sources();

    /* Where a name comes from, nullopt when no source names it. Throws app::Failure. */
    std::optional<Source> find(const std::string& name);

    /* The packages the sources name whose name matches pattern, as rant ls matches, each from
     * the first source that names it. Throws app::Failure. */
    std::vector<Offer> offers(const std::string& pattern = "");

    /* Why entries of the lists read so far name no package, one line each. */
    const std::vector<std::string>& problems() const { return problems_; }

    /* What sources.rant says, empty without one. */
    const config::Database& local() const { return local_; }

    /* The databases in the order they are read, the official one first. */
    std::vector<std::string> databases() const;

private:
    struct Found {
        Source      source;
        std::string description;
    };
    struct List {
        std::string                       named_by;
        std::optional<std::vector<Found>> found;
    };
    const std::vector<Found>& found(size_t i);

    config::Database         local_;
    std::vector<List>        lists_;
    std::vector<std::string> problems_;
};

/* The package a repo releases, as its package.rant at the default branch names it, for an
 * install by the repo's link. Throws app::Failure. */
Source from_repo(const std::string& repo);

/* What a package's package.rant says of it, at its newest release or as its folder has it. */
struct About {
    std::string              version;        /* the newest release, empty from a path */
    std::string              description;
    std::vector<std::string> files;          /* those of the newest release that install here */
};

/* What a release carries for a package on a platform, found by the names a release gives its
 * files: <name>-<version>-<platform>.tar.gz, unpacked into the package's folder,
 * <name>-<version>.vsix, installed into an editor, and <name>-<version>-skill.zip, unpacked
 * as the package's skill folder. */
struct Carried {
    const net::Asset*        archive = nullptr;
    const net::Asset*        extension = nullptr;
    const net::Asset*        skill = nullptr;
    std::vector<std::string> platforms;    /* those it has archives for */
};

Carried carried(const net::Release& r, const std::string& name, const std::string& platform);

/* Reads a source's package.rant, a repo's at its newest release. Throws app::Failure. */
About about(const Source& s);

/* What the sources named when last read, from the cache, so completion needs no network. With
 * read, the sources are read first when the official database never was. */
std::vector<Source> cached(bool read = false);

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

/* Removes an installed package, its extension from the editor that took it and the links to
 * its skill, or a library's skill. Throws app::Failure, also while one of its nodes runs. */
void remove(app::Context& ctx, const std::string& name);

/* The skill of a library, installed in ~/.rant/skills/<name> from the library's release. */
struct Skill {
    std::string           name;
    std::string           repo;
    std::string           version;
    std::vector<fs::path> links;    /* those that give it to AI coding agents */
};

std::optional<Skill> skill(const std::string& name);
std::vector<Skill>   skills();

/* Installs the <name>-<version>-skill.zip a library's release carries for the agents found, asking
 * first while none has it, only when may_ask. A given skill only moves forward. Throws app::Failure. */
void give_skill(const app::Context& ctx, const std::string& name, const std::string& repo, const net::Release& r, bool may_ask);

/* Drops each package that links a folder, so removing rant's home never reaches into it. */
void unlink_all();

/* Whether p is a symbolic link or, on Windows, a junction. */
bool is_link(const fs::path& p);

/* Removes such a link, never what it points to. Throws app::Failure. */
void unlink(const fs::path& p);

}
