#include "run/logs.hpp"

#include <fstream>

namespace run {

std::vector<std::string> tail(const config::fs::path& log, size_t n) {
    std::ifstream in(log, std::ios::binary | std::ios::ate);
    if (!in || n == 0) return {};
    const std::streamoff size = in.tellg();
    /* reads back from the end, twice as far each time, until n lines are in hand */
    for (std::streamoff window = 64 * 1024;; window *= 2) {
        std::streamoff from = window < size ? size - window : 0;
        std::string text((size_t)(size - from), '\0');
        in.seekg(from);
        in.read(text.data(), (std::streamsize)text.size());
        std::vector<std::string> lines;
        size_t start = 0;
        if (from > 0) start = text.find('\n') + 1;    /* the first line is cut off */
        while (start < text.size()) {
            size_t end = text.find('\n', start);
            if (end == std::string::npos) end = text.size();
            std::string line = text.substr(start, end - start);
            if (!line.empty() && line.back() == '\r') line.pop_back();
            lines.push_back(std::move(line));
            start = end + 1;
        }
        if (lines.size() >= n || from == 0) {
            if (lines.size() > n) lines.erase(lines.begin(), lines.end() - (std::ptrdiff_t)n);
            return lines;
        }
    }
}

}
