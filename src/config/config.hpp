#pragma once

#include <cstdint>
#include <filesystem>
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

struct Workspace {
    fs::path root;
    fs::path logs;
};

/* A workspace when one encloses the directory, and any errors met finding it. */
struct Opened {
    std::optional<Workspace> workspace;
    std::vector<Diagnostic>  diagnostics;
};

Opened open(const fs::path& start);
Opened init(const fs::path& dir);

fs::path from_utf8(const std::string& s);
std::string to_utf8(const fs::path& p);

}
