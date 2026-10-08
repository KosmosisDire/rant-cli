#include "ui/terminal.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <io.h>
#include <shellapi.h>
#else
#include <sys/ioctl.h>
#include <unistd.h>
#endif

namespace ui {

bool is_terminal(std::FILE* f) {
#ifdef _WIN32
    /* _isatty is true for the NUL device too, a console mode only a real console has */
    DWORD mode = 0;
    return GetConsoleMode((HANDLE)_get_osfhandle(_fileno(f)), &mode) != 0;
#else
    return isatty(fileno(f)) != 0;
#endif
}

bool enable_vt(std::FILE* f) {
    if (!is_terminal(f)) return false;
#ifdef _WIN32
    SetConsoleOutputCP(CP_UTF8);
    HANDLE h = GetStdHandle(f == stderr ? STD_ERROR_HANDLE : STD_OUTPUT_HANDLE);
    DWORD mode = 0;
    if (!GetConsoleMode(h, &mode)) return false;
    return SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0;
#else
    return true;
#endif
}

Size terminal_size() {
    Size s;
#ifdef _WIN32
    CONSOLE_SCREEN_BUFFER_INFO info;
    if (GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &info)) {
        s.rows = info.srWindow.Bottom - info.srWindow.Top + 1;
        s.cols = info.srWindow.Right - info.srWindow.Left + 1;
    }
#else
    struct winsize w;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == 0) {
        s.rows = w.ws_row;
        s.cols = w.ws_col;
    }
#endif
    return s;
}

#ifdef _WIN32
static std::string narrow(const wchar_t* w) {
    int n = WideCharToMultiByte(CP_UTF8, 0, w, -1, nullptr, 0, nullptr, nullptr);
    std::string s(n > 0 ? n - 1 : 0, '\0');
    if (n > 1) WideCharToMultiByte(CP_UTF8, 0, w, -1, s.data(), n, nullptr, nullptr);
    return s;
}
#endif

std::vector<std::string> utf8_args(int argc, char** argv) {
    std::vector<std::string> out;
#ifdef _WIN32
    (void)argc; (void)argv;
    int n = 0;
    wchar_t** w = CommandLineToArgvW(GetCommandLineW(), &n);
    for (int i = 1; w && i < n; i++) out.push_back(narrow(w[i]));
    LocalFree(w);
#else
    for (int i = 1; i < argc; i++) out.emplace_back(argv[i]);
#endif
    return out;
}

}
