#include <cctype>
#include <regex>

#include "app/failure.hpp"
#include "mesh/values.hpp"

namespace mesh {

/* A bare word: true, false, null, a number, or else text. */
static json word_value(const std::string& w) {
    if (w == "true") return true;
    if (w == "false") return false;
    if (w == "null") return nullptr;
    static const std::regex integer("[-+]?[0-9]+"), real("[-+]?([0-9]+[.]?[0-9]*|[.][0-9]+)([eE][-+]?[0-9]+)?");
    try {
        if (std::regex_match(w, integer)) return w[0] == '-' ? json(std::stoll(w)) : json(std::stoull(w));
        if (std::regex_match(w, real)) return std::stod(w);
    } catch (const std::out_of_range&) {
    }
    return w;
}

/* JSON with the quotes a shell makes awkward left optional: keys and words may be bare,
 * strings may use single quotes, and a trailing comma is fine. */
class Parser {
public:
    explicit Parser(std::string_view s) : s_(s) {}

    json whole() {
        json v = value();
        space();
        if (i_ < s_.size()) fail("unexpected text after the value");
        return v;
    }

private:
    [[noreturn]] void fail(const std::string& what) const {
        throw app::Failure("cannot read the value `" + std::string(s_) + "`: " + what + " at character " +
                           std::to_string(i_ + 1));
    }

    void space() {
        while (i_ < s_.size() && std::isspace((unsigned char)s_[i_])) i_++;
    }

    bool at(char c) {
        space();
        return i_ < s_.size() && s_[i_] == c;
    }

    json value() {
        space();
        if (i_ >= s_.size()) fail("a value is missing");
        char c = s_[i_];
        if (c == '{') return object();
        if (c == '[') return array();
        if (c == '"' || c == '\'') return quoted();
        return word_value(word());
    }

    std::string word() {
        size_t start = i_;
        while (i_ < s_.size() && !std::isspace((unsigned char)s_[i_]) && std::string_view(",:{}[]").find(s_[i_]) == std::string_view::npos)
            i_++;
        if (i_ == start) fail("expected a value");
        return std::string(s_.substr(start, i_ - start));
    }

    std::string quoted() {
        char q = s_[i_++];
        std::string out;
        while (i_ < s_.size() && s_[i_] != q) {
            char c = s_[i_++];
            if (c == '\\' && q == '"' && i_ < s_.size()) {
                char e = s_[i_++];
                switch (e) {
                case 'n': out += '\n'; break;
                case 't': out += '\t'; break;
                case 'r': out += '\r'; break;
                default: out += e; break;
                }
            } else {
                out += c;
            }
        }
        if (i_ >= s_.size()) fail("a quote is not closed");
        i_++;
        return out;
    }

    json object() {
        i_++;
        json o = json::object();
        while (!at('}')) {
            space();
            if (i_ >= s_.size()) fail("expected }");
            std::string key = (s_[i_] == '"' || s_[i_] == '\'') ? quoted() : word();
            if (!at(':')) fail("expected : after `" + key + "`");
            i_++;
            o[key] = value();
            if (at(',')) i_++;
            else if (!at('}')) fail("expected , or }");
        }
        i_++;
        return o;
    }

    json array() {
        i_++;
        json a = json::array();
        while (!at(']')) {
            if (i_ >= s_.size()) fail("expected ]");
            a.push_back(value());
            if (at(',')) i_++;
            else if (!at(']')) fail("expected , or ]");
        }
        i_++;
        return a;
    }

    std::string_view s_;
    size_t           i_ = 0;
};

json parse_text(std::string_view text) {
    size_t a = 0, b = text.size();
    while (a < b && std::isspace((unsigned char)text[a])) a++;
    while (b > a && std::isspace((unsigned char)text[b - 1])) b--;
    std::string_view t = text.substr(a, b - a);
    if (!t.empty() && std::string_view("{[\"'").find(t[0]) != std::string_view::npos) return Parser(t).whole();
    return word_value(std::string(t));    /* anything else is one word or plain text */
}

static bool assignment(const std::string& w) {
    static const std::regex path("[A-Za-z_][A-Za-z0-9_]*([.][A-Za-z_][A-Za-z0-9_]*|\\[[0-9]+\\])*=[\\s\\S]*");
    return std::regex_match(w, path);
}

Assignments parse_words(const std::vector<std::string>& words) {
    if (words.empty()) throw app::UsageError("give a value, such as 1.5, {x: 1, y: 2} or x=1 y=2");
    size_t n = 0;
    for (auto& w : words) n += assignment(w);
    Assignments out;
    if (n == words.size()) {
        for (auto& w : words) {
            size_t eq = w.find('=');
            out.emplace_back(w.substr(0, eq), parse_text(w.substr(eq + 1)));
        }
        return out;
    }
    if (n != 0) throw app::UsageError("give either one value or only field=value words, not both");
    std::string joined;
    for (auto& w : words) joined += (joined.empty() ? "" : " ") + w;
    out.emplace_back("", parse_text(joined));
    return out;
}

}
