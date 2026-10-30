#pragma once

#include <filesystem>
#include <string>
#include <vector>

/* The AI coding agents on this machine and the folders they read skills from, where rant
 * install links the skill a package carries. */
namespace agents {

namespace fs = std::filesystem;

struct Agent {
    std::string name;      /* "Claude Code" */
    fs::path    skills;    /* the folder of skills it reads */
};

/* The agents whose folder is in the home folder. */
std::vector<Agent> detected();

}
