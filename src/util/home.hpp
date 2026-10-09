#pragma once

#include <filesystem>
#include <string>

namespace util {

/* An environment variable as a path, empty when unset. Read wide on Windows. */
std::filesystem::path env_path(const char* name);

/* The user's home: HOME, else USERPROFILE on Windows. Empty when neither is set. */
std::filesystem::path home_dir();

/* ~/.rant, which holds the explorer, the mesh snapshots and the shell hooks. RANT_HOME
 * moves it, which the tests use. */
std::filesystem::path rant_home();

/* A stable name for a path: 16 hex digits of FNV-1a over it, case folded on Windows. */
std::string path_key(const std::filesystem::path& p);

}
