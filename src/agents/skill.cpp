#include "agents/skill.hpp"

#include <fstream>

#include "app/failure.hpp"
#include "scaffold/builtin.hpp"
#include "util/home.hpp"

namespace agents {

std::vector<Agent> detected() {
    fs::path home = util::home_dir();
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = home / ".config";
    /* Claude Code reads its own folder, the others share ~/.agents/skills */
    fs::path shared = home / ".agents" / "skills";
    const std::vector<std::pair<Agent, fs::path>> known = {
        { { "Claude Code", home / ".claude" / "skills" }, home / ".claude" },
        { { "Codex", shared }, home / ".codex" },
        { { "Cursor", shared }, home / ".cursor" },
        { { "Gemini CLI", shared }, home / ".gemini" },
        { { "GitHub Copilot", shared }, home / ".copilot" },
        { { "OpenCode", shared }, config / "opencode" },
    };
    std::vector<Agent> out;
    std::error_code ec;
    for (auto& [agent, folder] : known)
        if (fs::is_directory(folder, ec)) out.push_back(agent);
    return out;
}

bool installed(const fs::path& skills) {
    std::error_code ec;
    return fs::is_regular_file(skills / "rant" / "SKILL.md", ec);
}

fs::path install(const fs::path& skills) {
    fs::path dir = skills / "rant";
    std::error_code ec;
    fs::remove_all(dir, ec);
    static const std::string prefix = "skill/";
    for (auto& f : scaffold::builtin_files()) {
        std::string path = f.path;
        if (path.rfind(prefix, 0) != 0) continue;
        fs::path to = dir / fs::u8path(path.substr(prefix.size()));
        fs::create_directories(to.parent_path(), ec);
        std::ofstream out(to, std::ios::binary | std::ios::trunc);
        out.write((const char*)f.data, (std::streamsize)f.size);
        if (!out) throw app::Failure("cannot write " + to.u8string());
    }
    return dir;
}

}
