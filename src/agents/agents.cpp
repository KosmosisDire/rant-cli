#include "agents/agents.hpp"

#include "util/home.hpp"

namespace agents {

/* Each agent with the folder whose presence shows it is installed. */
static std::vector<std::pair<Agent, fs::path>> known() {
    fs::path home = util::home_dir();
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = home / ".config";
    /* Claude Code reads its own folder, the others share ~/.agents/skills */
    fs::path shared = home / ".agents" / "skills";
    return {
        { { "Claude Code", home / ".claude" / "skills" }, home / ".claude" },
        { { "Codex", shared }, home / ".codex" },
        { { "Cursor", shared }, home / ".cursor" },
        { { "Gemini CLI", shared }, home / ".gemini" },
        { { "GitHub Copilot", shared }, home / ".copilot" },
        { { "OpenCode", shared }, config / "opencode" },
    };
}

std::vector<Agent> detected() {
    std::vector<Agent> out;
    std::error_code ec;
    for (auto& [agent, folder] : known())
        if (fs::is_directory(folder, ec)) out.push_back(agent);
    return out;
}

}
