#pragma once

#include <filesystem>

namespace util {

/* The user's home: HOME, else USERPROFILE on Windows. Empty when neither is set. */
std::filesystem::path home_dir();

/* ~/.rant, which holds the explorer, the mesh snapshots and the shell hooks. RANT_HOME
 * moves it, which the tests use. */
std::filesystem::path rant_home();

}
