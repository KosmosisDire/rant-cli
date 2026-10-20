#pragma once

#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

/* The C++ face of the Rust config crate. Every call copies the crate's view into plain
 * C++ values and frees the handle, so nothing here points into Rust memory. */
namespace config {

namespace fs = std::filesystem;

fs::path from_utf8(const std::string& s);
std::string to_utf8(const fs::path& p);

struct Diagnostic {
    std::string file;
    uint32_t    line = 0;     /* 1 based, 0 = the whole file */
    uint32_t    column = 0;
    std::string message;

    std::string str() const;  /* "file:line:col: message" */
};

enum class NodeKind { Native, Python, CSharp, Declared, Rust };
const char* kind_name(NodeKind k);    /* "native", "python", "csharp", "declared", "rust" */

struct NodeType {
    std::string              package;
    std::string              name;
    NodeKind                 kind = NodeKind::Native;
    std::optional<fs::path>  path;      /* the artifact or source, none when declared */
    std::vector<std::string> run;       /* argv, never run through a shell */
    fs::path                 cwd;       /* the package root, or a loose file's folder */
    std::optional<fs::path>  source;    /* the file it is written in when the manifest says, as for Rust */

    std::string ref() const { return package.empty() ? name : package + "/" + name; }

    /* What a plan and the state call it: package/name, or its file outside every package. */
    std::string planned() const { return package.empty() && path ? to_utf8(*path) : ref(); }
};

struct Package {
    std::string              name;
    std::string              description;    /* what its package.rant says it is for */
    fs::path                 dir;
    std::vector<std::string> kinds;    /* "cmake", "python", "csharp", "rust", "package.rant" */
    std::vector<fs::path>    files;    /* its build files, then its package.rant */
    std::vector<NodeType>    nodes;
    bool                     installed = false;    /* put in rant's home by rant install, already built */
};

struct GroupFile {
    std::string name;          /* package/stem, or stem outside a package */
    std::string short_name;    /* the shortest name that finds it from where it was opened */
    fs::path    file;
};

struct Workspace {
    fs::path               root;
    fs::path               logs;
    fs::path               data;        /* the state file and caches */
    bool                   home = false;    /* rant's home outside every workspace, only installed packages */
    std::optional<uint16_t> domain;     /* the domain its config sets */
    std::vector<Package>   packages;    /* filled when opened with packages */
    std::vector<NodeType>  loose;       /* Python nodes outside every package, likewise */
    std::vector<GroupFile> groups;      /* filled when opened with packages */
};

/* A workspace when one encloses the directory, and any errors met reading it. */
struct Opened {
    std::optional<Workspace> workspace;
    std::vector<Diagnostic>  diagnostics;
};

/* One node of a resolved plan: its mesh name, its node type and exactly how it runs. */
struct Instance {
    std::string                        name;
    std::string                        type;    /* package/name, or the path of a loose file */
    NodeKind                           kind = NodeKind::Native;
    std::vector<std::string>           argv;
    std::map<std::string, std::string> env;
    fs::path                           cwd;
    std::string                        group;    /* the group whose own node block makes it, empty for a node run alone */
};

/* A resolved plan in start order. instances is empty whenever diagnostics is not. A group
 * plan names its group and every param it resolved, defaults included. */
struct Plan {
    std::optional<Workspace>           workspace;    /* root, logs and data only */
    std::string                        group;        /* empty for a single node */
    std::map<std::string, std::string> params;
    std::vector<Instance>              instances;
    std::vector<Diagnostic>            diagnostics;
};

/* What a name given to run, stop or restart means from start. Both and neither come with a
 * diagnostic that says so. */
struct Named {
    bool                    group = false;
    bool                    node  = false;
    std::vector<Diagnostic> diagnostics;
};

Named named(const fs::path& start, const std::string& name);

/* The plan for one node of a node type, a reference in any form resolved from start. */
Plan plan_node(const fs::path& start, const std::string& node_type);

/* The plan for a group by reference from start, with "key=value" params. */
Plan plan_group(const fs::path& start, const std::string& group, const std::vector<std::string>& params);

struct Param {
    std::string                name;
    std::string                type;
    std::optional<std::string> default_value;    /* none: the param is required */
    std::vector<std::string>   options;
    std::string                description;
};

/* A group with every param it takes, own and exposed, for help. */
struct GroupInfo {
    std::string              name;
    fs::path                 file;
    std::string              description;
    std::vector<Param>       params;
    std::vector<std::string> includes;    /* the groups it includes, by full name */
    std::vector<Diagnostic>  diagnostics;
};

GroupInfo describe_group(const fs::path& start, const std::string& group);

/* One package's build: the CMake configure when it needs one, then the commands, all in dir. */
struct BuildStep {
    std::string                           package;
    fs::path                              dir;
    std::vector<std::string>              configure;       /* the CMake configure, empty without CMake */
    bool                                  unconfigured = false;    /* configure runs first */
    fs::path                              build_dir;       /* the CMake build folder, empty without CMake */
    fs::path                              moved_from;      /* where the CMake cache was made, when not build_dir */
    std::vector<fs::path>                 clean;           /* what a clean build removes first */
    std::vector<std::vector<std::string>> commands;
};

/* `from` depends on `to`, found where `source` says. */
struct Edge {
    std::string from;
    std::string to;
    std::string source;
};

/* What a build runs, in order, and why that order. */
struct Build {
    std::vector<BuildStep>  steps;
    std::vector<Edge>       edges;
    std::vector<Diagnostic> diagnostics;
};

/* The build of the named packages and all they depend on, every package when none. */
Build plan_build(const fs::path& start, const std::vector<std::string>& packages);

/* The Python a folder runs with, by the interpreter rule node types follow. */
struct PythonInfo {
    std::optional<fs::path>  venv;           /* the nearest, up to the workspace root */
    fs::path                 venv_python;
    std::vector<std::string> interpreter;    /* the venv's python, else the system's */
    bool                     imports_rant = false;    /* one of the folder's own files does */
    std::optional<fs::path>  root;           /* the enclosing workspace's */
};

PythonInfo python_of(const fs::path& dir);

/* Every folder under start, start included, and every venv met on the way, which is not
 * entered. The workspace's ignore globs and rant's usual skips apply, .gitignore does not,
 * since a venv usually is in it. */
struct Folders {
    std::vector<fs::path> folders;
    std::vector<fs::path> venvs;
};

Folders folders_under(const fs::path& start);

/* A package.rant as one release carries it, read before that release is installed. */
struct PackageRelease {
    std::string             description;
    std::string             archive;    /* the release's files by kind, empty for a kind it lacks */
    std::string             vsix;
    std::vector<Diagnostic> diagnostics;
};

/* Reads package.rant text as if it were in dir, where the release installs, and names the
 * release's files for version and platform. */
PackageRelease read_release(const fs::path& dir, const std::string& text, const std::string& version, const std::string& platform);

/* One package the package database names, and its GitHub repo as owner/name. */
struct Listed {
    std::string name;
    std::string repo;
};

struct Database {
    std::vector<Listed>     packages;
    std::vector<Diagnostic> diagnostics;
};

/* Reads the package database's text. file only names it in errors. */
Database read_database(const std::string& file, const std::string& text);

/* rant's home, which holds the installed packages and stands in for a workspace outside
 * every one. Set once before anything is opened. */
void set_home(const fs::path& home);

/* Finds the enclosing workspace, else rant's home. With packages it also discovers every
 * package and scans it for node types, which reads the tree and is slower. */
Opened open(const fs::path& start, bool packages = false);
Opened init(const fs::path& dir);

}
