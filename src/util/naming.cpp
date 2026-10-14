#include "util/naming.hpp"

#include "rant.hpp"

namespace util {

std::string pascal(const std::string& name) {
    char out[256];
    size_t n = rant::detail::rant_type_name(name.c_str(), out, sizeof out);
    return n ? std::string(out, n) : name;    /* 0 for a name with no word in it */
}

std::string group_name(const std::string& name) {
    static const std::string suffix = "Group";
    std::string p = pascal(name);
    bool has = p.size() >= suffix.size() && p.compare(p.size() - suffix.size(), suffix.size(), suffix) == 0;
    return has ? p : p + suffix;
}

}
