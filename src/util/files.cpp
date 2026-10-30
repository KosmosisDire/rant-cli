#include "util/files.hpp"

#include <chrono>
#include <thread>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace util {

void rename(const std::filesystem::path& from, const std::filesystem::path& to, std::error_code& ec) {
    auto until = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    for (;;) {
        std::filesystem::rename(from, to, ec);
#ifdef _WIN32
        bool held = ec.category() == std::system_category() &&
                    (ec.value() == ERROR_ACCESS_DENIED || ec.value() == ERROR_SHARING_VIOLATION);
#else
        bool held = false;
#endif
        if (!held || std::chrono::steady_clock::now() >= until) return;
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
}

}
