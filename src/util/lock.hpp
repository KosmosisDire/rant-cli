#pragma once

#include <filesystem>

namespace util {

/* An exclusive lock on a file, held from construction to destruction. No started process
 * ever inherits it. */
class FileLock {
public:
    /* wait: block until the lock is free, else give up at once, and held() says which.
     * Throws app::Failure when the file cannot be opened. */
    FileLock(const std::filesystem::path& file, bool wait);
    ~FileLock();
    FileLock(const FileLock&) = delete;
    FileLock& operator=(const FileLock&) = delete;

    bool held() const { return held_; }

private:
#ifdef _WIN32
    void* handle_ = nullptr;
#else
    int   fd_ = -1;
#endif
    bool  held_ = false;
};

}
