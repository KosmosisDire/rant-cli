#include "util/home.hpp"

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

}
