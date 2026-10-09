#include "util/home.hpp"

#include <cctype>
#include <cstdio>
#include <cstdlib>

namespace util {

namespace fs = std::filesystem;

fs::path env_path(const char* name) {
#ifdef _WIN32
    std::wstring wide(name, name + std::char_traits<char>::length(name));
    const wchar_t* v = _wgetenv(wide.c_str());
#else
    const char* v = std::getenv(name);
#endif
    return v && *v ? fs::path(v) : fs::path();
}

fs::path home_dir() {
    fs::path home = env_path("HOME");
#ifdef _WIN32
    if (home.empty()) home = env_path("USERPROFILE");
#endif
    return home;
}

fs::path rant_home() {
    fs::path own = env_path("RANT_HOME");
    if (!own.empty()) return own;
    fs::path home = home_dir();
    return home.empty() ? fs::path() : home / ".rant";
}

std::string path_key(const fs::path& p) {
    std::string key = p.u8string();
#ifdef _WIN32
    for (auto& c : key) c = (char)std::tolower((unsigned char)c);
#endif
    uint64_t h = 1469598103934665603ull;
    for (unsigned char c : key) h = (h ^ c) * 1099511628211ull;
    char hex[17];
    std::snprintf(hex, sizeof hex, "%016llx", (unsigned long long)h);
    return hex;
}

}
