#pragma once

#include <filesystem>
#include <optional>

/* The explorer: the program of the installed explorer package, which explore opens and web
 * serves as a page. */
namespace commands::explorer {

/* The installed explorer's program, nullopt when the explorer package is not installed. */
std::optional<std::filesystem::path> program();

}
