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

enum class NodeKind { Native, Python, CSharp, Declared };
const char* kind_name(NodeKind k);    /* "native", "python", "csharp", "declared" */

struct NodeType {
    std::string              package;
    std::string              name;
    NodeKind                 kind = NodeKind::Native;
    std::optional<fs::path>  path;      /* the artifact or source, none when declared */
    std::vector<std::string> run;       /* argv, never run through a shell */
    fs::path                 cwd;       /* the package root, or a loose file's folder */

    std::string ref() const { return package.empty() ? name : package + "/" + name; }

    /* What a plan and the state call it: package/name, or its file outside every package. */
    std::string planned() const { return package.empty() && path ? to_utf8(*path) : ref(); }
};

struct Package {
    std::string              name;
    fs::path                 dir;
    std::vector<std::string> kinds;    /* "cmake", "python", "csharp", "rant.hcl" */
    std::vector<NodeType>    nodes;
};

struct GroupFile {
    std::string name;    /* package/stem, or stem outside a package */
    fs::path    file;
};

struct Workspace {
    fs::path               root;
    fs::path               logs;
    fs::path               data;        /* .rant/, the state file and caches */
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
    std::string             name;
    fs::path                file;
    std::string             description;
    std::vector<Param>      params;
    std::vector<Diagnostic> diagnostics;
};

GroupInfo describe_group(const fs::path& start, const std::string& group);

/* One package's build. configure, when not empty, runs first once the user agrees. */
struct BuildStep {
    std::string                           package;
    fs::path                              dir;
    std::vector<std::string>              configure;
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

/* Finds the enclosing workspace. With packages it also discovers every package and scans
 * it for node types, which reads the tree and is slower. */
Opened open(const fs::path& start, bool packages = false);
Opened init(const fs::path& dir);


}
