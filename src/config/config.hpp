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
    fs::path                 cwd;       /* the package root */

    std::string ref() const { return package.empty() ? name : package + "/" + name; }
};

struct Package {
    std::string           name;
    fs::path              dir;
    std::vector<NodeType> nodes;
};

struct Workspace {
    fs::path             root;
    fs::path             logs;
    fs::path             data;        /* .rant/, the state file and caches */
    std::vector<Package> packages;    /* filled when opened with packages */
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

/* A resolved plan in start order. instances is empty whenever diagnostics is not. */
struct Plan {
    std::optional<Workspace> workspace;    /* root, logs and data only */
    std::vector<Instance>    instances;
    std::vector<Diagnostic>  diagnostics;
};

/* The plan for one node of a node type, a reference in any form resolved from start. */
Plan plan_node(const fs::path& start, const std::string& node_type);

/* Finds the enclosing workspace. With packages it also discovers every package and scans
 * it for node types, which reads the tree and is slower. */
Opened open(const fs::path& start, bool packages = false);
Opened init(const fs::path& dir);

fs::path from_utf8(const std::string& s);
std::string to_utf8(const fs::path& p);

}
