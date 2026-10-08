#include "process/command.hpp"

#include <algorithm>
#include <cctype>
#include <cstdlib>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <unistd.h>
extern char** environ;
#endif

namespace process {

static bool has_separator(const std::string& s) {
#ifdef _WIN32
    return s.find_first_of("/\\") != std::string::npos;
#else
    return s.find('/') != std::string::npos;
#endif
}

static std::vector<std::string> split(const std::string& s, char sep) {
    std::vector<std::string> out;
    size_t start = 0;
    while (start <= s.size()) {
        size_t end = s.find(sep, start);
        if (end == std::string::npos) end = s.size();
        if (end > start) out.push_back(s.substr(start, end - start));
        start = end + 1;
    }
    return out;
}

#ifdef _WIN32
static std::string lower(std::string s) {
    for (auto& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}
#endif

/* A file the OS will run: on Windows any file, since the extension decides, and on POSIX
 * one with an execute bit. */
static bool runnable(const fs::path& p) {
    std::error_code ec;
    if (!fs::is_regular_file(p, ec)) return false;
#ifdef _WIN32
    return true;
#else
    return access(p.c_str(), X_OK) == 0;
#endif
}

#ifdef _WIN32
/* The executable extensions in PATHEXT order, lower case. */
static std::vector<std::string> path_exts() {
    const char* v = std::getenv("PATHEXT");
    auto exts = split(v && *v ? v : ".COM;.EXE;.BAT;.CMD", ';');
    for (auto& e : exts) e = lower(e);
    return exts;
}

/* With an executable extension the name is tried as given, else each PATHEXT extension. */
static std::optional<fs::path> with_exts(const fs::path& p) {
    auto exts = path_exts();
    std::string ext = lower(p.extension().u8string());
    if (!ext.empty() && std::find(exts.begin(), exts.end(), ext) != exts.end() && runnable(p)) return p;
    for (auto& e : exts) {
        fs::path c = p;
        c += fs::u8path(e);
        if (runnable(c)) return c;
    }
    return std::nullopt;
}
#endif

std::optional<fs::path> find_program(const std::string& program, const fs::path& cwd) {
    if (program.empty()) return std::nullopt;
    fs::path p = fs::u8path(program);
    if (has_separator(program) || p.is_absolute()) {
        if (p.is_relative()) p = cwd / p;
#ifdef _WIN32
        return with_exts(p);
#else
        return runnable(p) ? std::optional<fs::path>(p) : std::nullopt;
#endif
    }
    const char* path = std::getenv("PATH");
#ifdef _WIN32
    for (auto& dir : split(path ? path : "", ';'))
        if (auto f = with_exts(fs::u8path(dir) / p)) return f;
#else
    for (auto& dir : split(path ? path : "", ':')) {
        fs::path c = fs::u8path(dir) / p;
        if (runnable(c)) return c;
    }
#endif
    return std::nullopt;
}

static std::string env_key(const std::string& name) {
#ifdef _WIN32
    return lower(name);
#else
    return name;
#endif
}

std::vector<std::string> environment(const std::map<std::string, std::string>& env) {
    std::map<std::string, std::string> merged;    /* key, then the entry as written */
#ifdef _WIN32
    wchar_t* block = GetEnvironmentStringsW();
    for (wchar_t* e = block; e && *e; e += wcslen(e) + 1) {
        int n = WideCharToMultiByte(CP_UTF8, 0, e, -1, nullptr, 0, nullptr, nullptr);
        std::string s(n > 0 ? n - 1 : 0, '\0');
        if (n > 1) WideCharToMultiByte(CP_UTF8, 0, e, -1, s.data(), n, nullptr, nullptr);
        size_t eq = s.find('=', 1);    /* "=C:=C:\x" entries start with '=' */
        merged[env_key(s.substr(0, eq))] = s;
    }
    if (block) FreeEnvironmentStringsW(block);
#else
    for (char** e = environ; e && *e; e++) {
        std::string s = *e;
        merged[env_key(s.substr(0, s.find('=')))] = s;
    }
#endif
    for (auto& [k, v] : env) merged[env_key(k)] = k + "=" + v;
    std::vector<std::string> out;
    for (auto& [k, v] : merged) out.push_back(v);
    return out;
}

std::string shown(const std::vector<std::string>& argv) {
    std::string s;
    for (auto& a : argv) {
        if (!s.empty()) s += ' ';
        bool quote = a.empty() || a.find_first_of(" \t\"'") != std::string::npos;
        s += quote ? "'" + a + "'" : a;
    }
    return s;
}

}
