#include "app/args.hpp"

#include <cctype>

#include "app/failure.hpp"

namespace app {

std::optional<std::string> Args::get(std::string_view name) const {
    auto it = options_.find(std::string(name));
    if (it == options_.end()) return std::nullopt;
    return it->second;
}

void Args::merge(const Args& other) {
    for (auto& [k, v] : other.options_) options_.emplace(k, v);
}

bool looks_like_number(std::string_view s) {
    if (!s.empty() && (s[0] == '-' || s[0] == '+')) s.remove_prefix(1);
    if (s.empty()) return false;
    bool digit = false;
    for (char c : s) {
        if (std::isdigit((unsigned char)c)) digit = true;
        else if (c != '.' && c != 'e' && c != 'E' && c != '-' && c != '+') return false;
    }
    return digit;
}

static const OptionSpec* find_long(const std::vector<OptionSpec>& specs, std::string_view name) {
    for (auto& s : specs)
        if (s.name == name) return &s;
    return nullptr;
}

static const OptionSpec* find_short(const std::vector<OptionSpec>& specs, char c) {
    for (auto& s : specs)
        if (s.short_name == c) return &s;
    return nullptr;
}

Args parse_args(const std::vector<std::string>& tokens, const std::vector<OptionSpec>& specs,
                bool stop_at_word, size_t* consumed) {
    Args args;
    bool options_done = false;
    size_t i = 0;
    for (; i < tokens.size(); i++) {
        const std::string& t = tokens[i];
        bool is_option = !options_done && t.size() > 1 && t[0] == '-' && !looks_like_number(t);
        if (!is_option) {
            if (stop_at_word) break;
            args.words.push_back(t);
            continue;
        }
        if (t == "--") { options_done = true; continue; }

        const OptionSpec* spec = nullptr;
        std::string inline_value;
        bool has_inline = false;
        if (t.rfind("--", 0) == 0) {
            std::string_view body(t);
            body.remove_prefix(2);
            size_t eq = body.find('=');
            if (eq != std::string_view::npos) {
                inline_value = std::string(body.substr(eq + 1));
                has_inline = true;
                body = body.substr(0, eq);
            }
            spec = find_long(specs, body);
            if (!spec) throw UsageError("unknown option --" + std::string(body));
        } else {
            if (t.size() != 2) throw UsageError("unknown option " + t);
            spec = find_short(specs, t[1]);
            if (!spec) throw UsageError("unknown option " + t);
        }

        if (spec->value_name.empty()) {
            if (has_inline) throw UsageError("--" + std::string(spec->name) + " takes no value");
            args.set(spec->name, "");
        } else if (has_inline) {
            args.set(spec->name, inline_value);
        } else if (i + 1 < tokens.size()) {
            args.set(spec->name, tokens[++i]);
        } else {
            throw UsageError("--" + std::string(spec->name) + " needs a value");
        }
    }
    if (consumed) *consumed = i;
    return args;
}

}
