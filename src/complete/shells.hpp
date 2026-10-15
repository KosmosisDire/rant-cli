#pragma once

#include <filesystem>
#include <functional>
#include <string>
#include <string_view>
#include <vector>

/* The small per shell hooks that hand tab completion to `rant __complete`. */
namespace complete {

struct Shell {
    std::string_view name;    /* "bash", "zsh", "fish", "powershell", "pwsh" */
    std::string_view hook;    /* the script the shell loads */
};

const std::vector<Shell>& shells();
const Shell* find_shell(std::string_view name);

/* The shells found on this machine: PowerShell on Windows, else bash, zsh and fish on PATH. */
std::vector<const Shell*> detected();

/* Writes the hook under ~/.rant and makes the shell load it, once however often it runs.
 * Returns where it loads from. Throws app::Failure. */
std::string install(const Shell& shell);

/* One thing setup or an install script added outside ~/.rant: where it is, what part of
 * that it is when not all of it, and how to take it out again. */
struct Added {
    std::filesystem::path path;
    std::string           part;
    std::function<void()> remove;
};

/* What is there now of the lines and files that load rant into shells and put it on PATH. */
std::vector<Added> added();

}
