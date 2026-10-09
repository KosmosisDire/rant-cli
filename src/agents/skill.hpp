#pragma once

#include <filesystem>
#include <string>
#include <vector>

/* The rant skill for AI coding agents: templates/skill, compiled in and written where each
 * agent found on this machine reads its skills. */
namespace agents {

namespace fs = std::filesystem;

struct Agent {
    std::string name;      /* "Claude Code" */
    fs::path    skills;    /* the folder of skills it reads */
};

/* The agents whose folder is in the home folder. The installers check the same folders. */
std::vector<Agent> detected();

/* Writes the skill as rant/ in skills, replacing what an older one left. Returns where.
 * Throws app::Failure. */
fs::path install(const fs::path& skills);

}
