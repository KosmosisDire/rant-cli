#include "ui/yaml.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstdlib>

namespace ui {

using json = nlohmann::ordered_json;

Yaml::Yaml(const Output* out, bool stable) : out_(out), stable_(stable) {}

std::string Yaml::paint(Style s, const std::string& text) const {
    return out_ ? out_->paint(s, text) : text;
}

/* A value that fits after its key on one line: a scalar, an empty container, or a list of
 * scalars, which reads best as [a, b]. */
static bool one_line(const json& v) {
    if (!v.is_structured() || v.empty()) return true;
    if (v.is_object()) return false;
    return std::none_of(v.begin(), v.end(), [](const json& e) { return e.is_structured(); });
}

static bool looks_like_number(const std::string& s) {
    char* end = nullptr;
    std::strtod(s.c_str(), &end);
    return end && *end == '\0';
}

/* Whether text would read back as something else, or break the structure, unquoted. */
static bool needs_quotes(const std::string& s, bool in_flow) {
    if (s.empty() || looks_like_number(s)) return true;
    std::string lower = s;
    for (auto& c : lower) c = (char)std::tolower((unsigned char)c);
    static const char* words[] = { "true", "false", "null", "~", "yes", "no", "on", "off", ".inf", ".nan" };
    for (auto* w : words)
        if (lower == w) return true;
    if (std::string(" -?:,[]{}#&*!|>'\"%@`").find(s.front()) != std::string::npos || s.back() == ' ') return true;
    if (s.find(": ") != std::string::npos || s.find(" #") != std::string::npos || s.back() == ':') return true;
    for (unsigned char c : s)
        if (c < 0x20 || c == 0x7f || (in_flow && std::string(",[]{}").find((char)c) != std::string::npos)) return true;
    return false;
}

static std::string quoted(const std::string& s) {
    std::string out = "\"";
    for (unsigned char c : s) {
        switch (c) {
        case '"':  out += "\\\""; break;
        case '\\': out += "\\\\"; break;
        case '\n': out += "\\n"; break;
        case '\t': out += "\\t"; break;
        case '\r': out += "\\r"; break;
        default:
            if (c < 0x20 || c == 0x7f) {
                char hex[8];
                std::snprintf(hex, sizeof hex, "\\x%02x", c);
                out += hex;
            } else {
                out += (char)c;
            }
        }
    }
    return out + "\"";
}

static std::string text_of(const std::string& s, bool in_flow) { return needs_quotes(s, in_flow) ? quoted(s) : s; }

/* A number as shown: floats to two decimals, a rounded zero without its minus sign. */
static std::string number_text(const json& v) {
    if (v.is_number_unsigned()) return std::to_string(v.get<uint64_t>());
    if (v.is_number_integer()) return std::to_string(v.get<int64_t>());
    double d = v.get<double>();
    if (std::isnan(d)) return ".nan";
    if (std::isinf(d)) return d < 0 ? "-.inf" : ".inf";
    char buf[64];
    std::snprintf(buf, sizeof buf, "%.2f", d);
    std::string s = buf;
    if (s == "-0.00") s = "0.00";
    return s;
}

std::string Yaml::scalar(const json& v, const std::string& path, bool in_flow) {
    if (v.is_null()) return paint(Style::Faint, "null");
    if (v.is_boolean()) return paint(Style::Amber, v.get<bool>() ? "true" : "false");
    if (v.is_string()) return paint(Style::GreenHi, text_of(v.get<std::string>(), in_flow));
    std::string t = number_text(v);
    if (stable_) {
        if (t[0] != '-') t = " " + t;    /* the slot a minus sign takes */
        size_t& w = widths_[path];
        w = std::max(w, t.size());
        t.insert(0, w - t.size(), ' ');
    }
    return paint(Style::Plain, t);
}

std::string Yaml::inline_value(const json& v, const std::string& path, bool in_flow) {
    if (!v.is_structured()) return scalar(v, path, in_flow);
    if (v.empty()) return paint(Style::Faint, v.is_array() ? "[]" : "{}");
    std::string out = paint(Style::Faint, "[");
    bool first = true;
    for (auto& e : v) {
        if (!first) out += paint(Style::Faint, ",") + " ";
        first = false;
        out += e.is_structured() ? flow_at(e, path + "[]") : scalar(e, path + "[]", true);
    }
    return out + paint(Style::Faint, "]");
}

std::string Yaml::key(const std::string& k, size_t key_width) {
    std::string text = text_of(k, false);
    return paint(Style::Dim, text) + paint(Style::Faint, ":") + std::string(key_width > text.size() ? key_width - text.size() : 0, ' ');
}

std::string Yaml::entry(const std::string& k, const json& v, size_t key_width) {
    std::string out;
    block_into(out, json{ { k, v } }, 0, "", key_width);
    return out;
}

void Yaml::block_into(std::string& out, const json& v, size_t indent, const std::string& path, size_t key_width) {
    std::string pad(indent, ' ');
    if (v.is_object()) {
        size_t widest = key_width;
        for (auto& [k, _] : v.items()) widest = std::max(widest, text_of(k, false).size());
        for (auto& [k, e] : v.items()) {
            std::string child = path.empty() ? k : path + "." + k;
            if (!out.empty()) out += "\n";
            if (one_line(e)) {
                out += pad + key(k, widest) + " " + inline_value(e, child, false);
            } else {
                out += pad + key(k, 0) + "\n";
                std::string inner;
                block_into(inner, e, indent + 2, child, 0);
                out += inner;
            }
        }
        return;
    }
    for (auto& e : v) {    /* a list holding objects or lists, one item per dash */
        if (!out.empty()) out += "\n";
        std::string item;
        if (e.is_object() && !e.empty()) {
            block_into(item, e, indent + 2, path + "[]", 0);
            item.erase(0, indent + 2);    /* the first key sits on the dash line */
        } else {
            item = inline_value(e, path + "[]", false);
        }
        out += pad + paint(Style::Faint, "-") + " " + item;
    }
}

std::string Yaml::block(const json& v) {
    if ((one_line(v) && !v.is_object()) || v.empty()) return inline_value(v, "", false);
    std::string out;
    block_into(out, v, 0, "", 0);
    return out;
}

std::string Yaml::flow(const json& v) { return flow_at(v, ""); }

std::string Yaml::flow_at(const json& v, const std::string& path) {
    if (!v.is_object()) return inline_value(v, path, true);
    std::string out = paint(Style::Faint, "{");
    bool first = true;
    for (auto& [k, e] : v.items()) {
        if (!first) out += paint(Style::Faint, ",") + " ";
        first = false;
        std::string child = path.empty() ? k : path + "." + k;
        out += paint(Style::Dim, text_of(k, true)) + paint(Style::Faint, ":") + " ";
        out += e.is_object() ? flow_at(e, child) : inline_value(e, child, true);
    }
    return out + paint(Style::Faint, "}");
}

}
