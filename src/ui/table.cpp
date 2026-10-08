#include "ui/table.hpp"

#include <algorithm>

namespace ui {

size_t visible_width(const std::string& s) {
    size_t w = 0;
    for (size_t i = 0; i < s.size(); i++) {
        if (s[i] == '\x1b') {
            while (i < s.size() && s[i] != 'm') i++;
            continue;
        }
        if (((unsigned char)s[i] & 0xC0) != 0x80) w++;    /* count UTF-8 lead bytes only */
    }
    return w;
}

std::vector<std::string> Table::lines(const std::string& indent) const {
    std::vector<size_t> widths;
    for (auto& r : rows_) {
        if (widths.size() < r.size()) widths.resize(r.size(), 0);
        for (size_t i = 0; i < r.size(); i++) widths[i] = std::max(widths[i], visible_width(r[i]));
    }
    std::vector<std::string> out;
    for (auto& r : rows_) {
        std::string line = indent;
        for (size_t i = 0; i < r.size(); i++) {
            line += r[i];
            if (i + 1 < r.size()) line += std::string(widths[i] - visible_width(r[i]) + 2, ' ');
        }
        out.push_back(line);
    }
    return out;
}

}
