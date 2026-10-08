#include "ui/csv.hpp"

#include <map>

namespace ui {

using json = nlohmann::ordered_json;

static void flatten(const json& v, const std::string& path, std::vector<std::pair<std::string, const json*>>& out) {
    if (v.is_object()) {
        for (auto& [k, e] : v.items()) flatten(e, path.empty() ? k : path + "." + k, out);
    } else if (v.is_array()) {
        for (size_t i = 0; i < v.size(); i++) flatten(v[i], path + "[" + std::to_string(i) + "]", out);
    } else {
        out.emplace_back(path.empty() ? "value" : path, &v);
    }
}

/* RFC 4180: a field holding a comma, a quote or a line break is quoted, quotes doubled. */
static std::string field(const std::string& s) {
    if (s.find_first_of(",\"\r\n") == std::string::npos) return s;
    std::string out = "\"";
    for (char c : s) out += c == '"' ? std::string("\"\"") : std::string(1, c);
    return out + "\"";
}

static std::string cell(const json& v) {
    if (v.is_null()) return "";
    if (v.is_string()) return field(v.get<std::string>());
    return v.dump();
}

std::string Csv::rows(const json& v) {
    std::vector<std::pair<std::string, const json*>> leaves;
    flatten(v, "", leaves);
    std::string out;
    if (columns_.empty()) {
        for (auto& [path, _] : leaves) {
            columns_.push_back(path);
            out += (out.empty() ? "" : ",") + field(path);
        }
        out += "\n";
    }
    std::map<std::string, const json*> by_path(leaves.begin(), leaves.end());
    std::string row;
    for (size_t i = 0; i < columns_.size(); i++) {
        if (i) row += ",";
        auto it = by_path.find(columns_[i]);
        if (it != by_path.end()) row += cell(*it->second);
    }
    return out + row;
}

}
