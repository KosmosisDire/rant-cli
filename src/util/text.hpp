#pragma once

#include <string>
#include <vector>

namespace util {

/* Words for a message, "a, b, c" by default. */
inline std::string joined(const std::vector<std::string>& v, const char* sep = ", ") {
    std::string s;
    for (auto& x : v) s += (s.empty() ? "" : sep) + x;
    return s;
}

}
