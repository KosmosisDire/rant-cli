#include "mesh/snapshot.hpp"

#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "mesh/client.hpp"
#include "util/home.hpp"

namespace mesh {

namespace fs = std::filesystem;
using json = nlohmann::json;

static fs::path file_of(uint16_t domain) {
    fs::path home = util::rant_home();
    return home.empty() ? fs::path() : home / "cache" / ("mesh-" + std::to_string(domain) + ".json");
}

Snapshot Snapshot::load(uint16_t domain) {
    Snapshot s;
    fs::path file = file_of(domain);
    if (file.empty()) return s;
    std::ifstream in(file, std::ios::binary);
    json j = json::parse(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>(), nullptr, false);
    if (!j.is_object()) return s;
    for (auto& n : j.value("nodes", json::array()))
        if (n.is_string()) s.nodes.insert(n.get<std::string>());
    for (auto& e : j.value("entities", json::array()))
        if (e.is_object() && e.value("name", json()).is_string() && e.value("kind", json()).is_number_integer())
            s.entities.insert({ e["name"].get<std::string>(), (rant::EntityKind)e["kind"].get<int>() });
    return s;
}

void Snapshot::save(uint16_t domain) const {
    fs::path file = file_of(domain);
    if (file.empty()) return;
    json j = { { "nodes", json::array() }, { "entities", json::array() } };
    for (auto& n : nodes) j["nodes"].push_back(n);
    for (auto& [name, kind] : entities) j["entities"].push_back({ { "name", name }, { "kind", (int)kind } });
    std::error_code ec;
    fs::create_directories(file.parent_path(), ec);
    fs::path tmp = file;
    tmp += ".tmp";
    {
        std::ofstream out(tmp, std::ios::binary | std::ios::trunc);
        out << j.dump();
        if (!out) return;
    }
    fs::rename(tmp, file, ec);
}

void Snapshot::add(const Client& mesh) {
    for (auto& p : mesh.peers()) nodes.insert(p.name);
    for (auto& e : mesh.entities())
        if (!is_placeholder(e.name)) entities.insert({ e.name, e.kind });
}

}
