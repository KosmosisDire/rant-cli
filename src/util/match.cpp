#include "util/match.hpp"

namespace util {

bool glob(std::string_view p, std::string_view t) {
    size_t pi = 0, ti = 0, star = std::string_view::npos, mark = 0;
    while (ti < t.size()) {
        if (pi < p.size() && (p[pi] == '?' || p[pi] == t[ti])) { pi++; ti++; }
        else if (pi < p.size() && p[pi] == '*') { star = pi++; mark = ti; }
        else if (star != std::string_view::npos) { pi = star + 1; ti = ++mark; }
        else return false;
    }
    while (pi < p.size() && p[pi] == '*') pi++;
    return pi == p.size();
}

bool name_matches(std::string_view pattern, std::string_view name) {
    if (pattern.empty()) return true;
    if (pattern.find_first_of("*?") != std::string_view::npos) return glob(pattern, name);
    return name.find(pattern) != std::string_view::npos;
}

}
