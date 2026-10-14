#include "run/logs.hpp"

#include <fstream>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <sys/stat.h>
#endif

namespace run {

namespace fs = config::fs;

static std::vector<std::string> split_lines(const std::string& text, size_t start) {
    std::vector<std::string> lines;
    while (start < text.size()) {
        size_t end = text.find('\n', start);
        if (end == std::string::npos) end = text.size();
        std::string line = text.substr(start, end - start);
        if (!line.empty() && line.back() == '\r') line.pop_back();
        lines.push_back(std::move(line));
        start = end + 1;
    }
    return lines;
}

std::vector<std::string> tail(const fs::path& log, size_t n) {
    std::ifstream in(log, std::ios::binary | std::ios::ate);
    if (!in || n == 0) return {};
    const std::streamoff size = in.tellg();
    /* reads back from the end, twice as far each time, until n lines are in hand */
    for (std::streamoff window = 64 * 1024;; window *= 2) {
        std::streamoff from = window < size ? size - window : 0;
        std::string text((size_t)(size - from), '\0');
        in.seekg(from);
        in.read(text.data(), (std::streamsize)text.size());
        size_t first = from > 0 ? text.find('\n') + 1 : 0;    /* a cut off first line is dropped */
        std::vector<std::string> lines = split_lines(text, first);
        if (lines.size() >= n || from == 0) {
            if (lines.size() > n) lines.erase(lines.begin(), lines.end() - (std::ptrdiff_t)n);
            return lines;
        }
    }
}

std::vector<std::string> logged_nodes(const fs::path& logs) {
    std::vector<std::string> out;
    std::error_code ec;
    for (fs::recursive_directory_iterator it(logs, ec), end; !ec && it != end; it.increment(ec)) {
        if (it->path().extension() != ".log") continue;
        fs::path rel = it->path().lexically_relative(logs);
        rel.replace_extension();
        out.push_back(rel.generic_u8string());
    }
    return out;
}

/* What tells one file from another of the same path, empty when there is none. */
static std::string file_id(const fs::path& p) {
#ifdef _WIN32
    HANDLE h = CreateFileW(p.wstring().c_str(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                           nullptr, OPEN_EXISTING, 0, nullptr);
    if (h == INVALID_HANDLE_VALUE) return "";
    BY_HANDLE_FILE_INFORMATION info;
    BOOL ok = GetFileInformationByHandle(h, &info);
    CloseHandle(h);
    if (!ok) return "";
    return std::to_string(info.dwVolumeSerialNumber) + ":" + std::to_string(((uint64_t)info.nFileIndexHigh << 32) | info.nFileIndexLow);
#else
    struct stat st;
    if (stat(p.c_str(), &st) != 0) return "";
    return std::to_string((uint64_t)st.st_dev) + ":" + std::to_string((uint64_t)st.st_ino);
#endif
}

Follower::Follower(fs::path log) : path_(std::move(log)), id_(file_id(path_)) {
    std::error_code ec;
    if (!id_.empty()) offset_ = fs::file_size(path_, ec);
}

std::vector<std::string> Follower::poll() {
    std::string id = file_id(path_);
    if (id.empty()) return {};    /* between the move and the new log */
    if (id != id_) id_ = id, offset_ = 0, partial_.clear();
    std::error_code ec;
    uint64_t size = fs::file_size(path_, ec);
    if (ec) return {};
    if (size < offset_) offset_ = 0, partial_.clear();    /* truncated */
    if (size == offset_) return {};
    std::ifstream in(path_, std::ios::binary);
    if (!in) return {};
    std::string text(size - offset_, '\0');
    in.seekg((std::streamoff)offset_);
    in.read(text.data(), (std::streamsize)text.size());
    text.resize((size_t)in.gcount());
    offset_ += text.size();
    partial_ += text;
    size_t last = partial_.rfind('\n');
    if (last == std::string::npos) return {};
    std::vector<std::string> lines = split_lines(partial_.substr(0, last + 1), 0);
    partial_.erase(0, last + 1);
    return lines;
}

}
