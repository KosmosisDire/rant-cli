#include "scaffold/scaffold.hpp"

#include <fstream>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "scaffold/builtin.hpp"

namespace scaffold {

/* A preset and the template folders whose files it writes. */
struct Preset {
    const char*              name;
    std::vector<std::string> folders;
};

static const std::vector<Preset> presets = {
    { "node-cpp", { "node-cpp" } },
    { "node-python", { "node-python" } },
    { "package-cpp", { "package-cpp", "node-cpp" } },
    { "package-python", { "package-python", "node-python" } },
    { "package-csharp", { "package-csharp" } },
    { "group", { "group" } },
};

static std::string with_name(std::string text, const std::string& name) {
    static const std::string token = "{{name}}";
    for (size_t at = text.find(token); at != std::string::npos; at = text.find(token, at + name.size()))
        text.replace(at, token.size(), name);
    return text;
}

Files plan(const std::string& preset, const fs::path& dest, const std::string& name) {
    const Preset* p = nullptr;
    for (auto& candidate : presets)
        if (candidate.name == preset) p = &candidate;
    if (!p) throw app::Failure("no preset `" + preset + "`");

    Files files;
    for (auto& folder : p->folders) {
        std::string prefix = folder + "/";
        for (auto& f : builtin_files()) {
            std::string path = f.path;
            if (path.rfind(prefix, 0) != 0) continue;
            fs::path to = dest / fs::u8path(with_name(path.substr(prefix.size()), name));
            files.push_back({ to, with_name(std::string((const char*)f.data, f.size), name) });
        }
    }
    for (auto& [path, _] : files)
        if (fs::exists(path)) throw app::Failure(config::to_utf8(path) + " exists already, nothing was written");
    return files;
}

void write(const Files& files) {
    for (auto& [path, text] : files) {
        std::error_code ec;
        fs::create_directories(path.parent_path(), ec);
        std::ofstream out(path, std::ios::binary | std::ios::trunc);
        out << text;
        if (!out) throw app::Failure("cannot write " + config::to_utf8(path));
    }
}

}
