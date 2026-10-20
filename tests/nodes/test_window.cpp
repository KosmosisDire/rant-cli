/* A Windows GUI program for the end to end tests, built as the bridge is: no console, and a
 * hidden window whose closing stops it, which is how rant stop asks such a program.
 *
 *   test_window [--life SECONDS]
 *
 * Prints "ready test_window" once its window is up and "stopped" when the window is closed. */

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>

static LRESULT CALLBACK on_message(HWND w, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_CLOSE) {
        std::printf("stopped\n");
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(w, msg, wp, lp);
}

int main(int argc, char** argv) {
    double life = 60;
    for (int i = 1; i + 1 < argc; i++)
        if (!std::strcmp(argv[i], "--life")) life = std::atof(argv[++i]);
    std::setvbuf(stdout, nullptr, _IONBF, 0);

    WNDCLASSW wc{};
    wc.lpfnWndProc = on_message;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"rant_test_window";
    RegisterClassW(&wc);
    HWND window = CreateWindowExW(0, wc.lpszClassName, L"test_window", 0, 0, 0, 0, 0, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    std::printf("ready test_window\n");

    /* a failed test must not leave it running long */
    SetTimer(window, 1, (UINT)(life * 1000), nullptr);
    MSG m;
    while (GetMessageW(&m, nullptr, 0, 0) > 0) {
        if (m.message == WM_TIMER) return 1;
        DispatchMessageW(&m);
    }
    return 0;
}
