#pragma once

#include <map>
#include <string>

#include <nlohmann/json.hpp>

#include "ui/output.hpp"

namespace ui {

/* Data as YAML for a person to read: keys aligned, floats to two decimals, and colored
 * in the explorer's palette when the output is a terminal. JSON and CSV keep the full
 * values. */
class Yaml {
public:
    /* out paints, nullptr prints plain. stable gives every number a sign slot and keeps
     * each field at the widest width it has needed, so a value updated in place or a line
     * per message never shifts as the numbers change. */
    Yaml(const Output* out, bool stable);

    /* Block style over as many lines as it takes, with no newline at the end. */
    std::string block(const nlohmann::ordered_json& v);

    /* Flow style on one line: {x: 1.00, tags: [a, b]}. */
    std::string flow(const nlohmann::ordered_json& v);

    /* One mapping entry in block style, its key padded to key_width so a caller building
     * a mapping entry by entry lines its values up. */
    std::string entry(const std::string& key, const nlohmann::ordered_json& v, size_t key_width);

    /* A key as an entry writes it, padded to key_width, colon included. */
    std::string key(const std::string& key, size_t key_width);

private:
    std::string flow_at(const nlohmann::ordered_json& v, const std::string& path);
    void block_into(std::string& out, const nlohmann::ordered_json& v, size_t indent, const std::string& path,
                    size_t key_width);
    std::string inline_value(const nlohmann::ordered_json& v, const std::string& path, bool in_flow);
    std::string scalar(const nlohmann::ordered_json& v, const std::string& path, bool in_flow);
    std::string paint(Style s, const std::string& text) const;

    const Output*                 out_;
    bool                          stable_;
    std::map<std::string, size_t> widths_;
};

}
