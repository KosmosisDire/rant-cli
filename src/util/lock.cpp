#include "util/lock.hpp"

#include "app/failure.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

namespace util {

FileLock::FileLock(const std::filesystem::path& file, bool wait) {
#ifdef _WIN32
    /* Opened without inheritance, so no node ever holds the lock. */
    HANDLE h = CreateFileW(file.wstring().c_str(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                           nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) throw app::Failure("cannot open " + file.u8string());
    OVERLAPPED ov{};
    held_ = LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK | (wait ? 0 : LOCKFILE_FAIL_IMMEDIATELY), 0, 1, 0, &ov) != 0;
    if (!held_ && (wait || GetLastError() != ERROR_LOCK_VIOLATION)) {
        CloseHandle(h);
        throw app::Failure("cannot lock " + file.u8string());
    }
    handle_ = h;
#else
    /* Close on exec, so no node ever holds the lock. */
    fd_ = open(file.c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0644);
    if (fd_ < 0) throw app::Failure("cannot open " + file.string() + ": " + std::strerror(errno));
    for (;;) {
        if (flock(fd_, LOCK_EX | (wait ? 0 : LOCK_NB)) == 0) {
            held_ = true;
            break;
        }
        if (errno == EINTR) continue;
        if (!wait && errno == EWOULDBLOCK) break;
        int saved = errno;
        close(fd_);
        fd_ = -1;
        throw app::Failure("cannot lock " + file.string() + ": " + std::strerror(saved));
    }
#endif
}

FileLock::~FileLock() {
#ifdef _WIN32
    if (handle_) {
        if (held_) {
            OVERLAPPED ov{};
            UnlockFileEx(handle_, 0, 1, 0, &ov);
        }
        CloseHandle(handle_);
    }
#else
    if (fd_ >= 0) close(fd_);    /* closing drops the flock */
#endif
}

}
