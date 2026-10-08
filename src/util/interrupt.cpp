#include "util/interrupt.hpp"

#include <atomic>
#include <csignal>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace util {

static std::atomic<bool> flag{ false };

#ifdef _WIN32
static BOOL WINAPI on_ctrl(DWORD type) {
    if (type == CTRL_C_EVENT || type == CTRL_BREAK_EVENT) {
        flag = true;
        return TRUE;
    }
    return FALSE;
}
#else
static void on_signal(int) { flag = true; }
#endif

void catch_interrupt() {
#ifdef _WIN32
    SetConsoleCtrlHandler(on_ctrl, TRUE);
#else
    std::signal(SIGINT, on_signal);
    std::signal(SIGTERM, on_signal);
#endif
}

bool interrupted() { return flag; }

}
