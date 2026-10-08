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

/* The widths of each part of each column when cells go down rows at a time. */
static std::vector<std::vector<size_t>> column_widths(const std::vector<std::vector<std::string>>& cells, size_t rows) {
    std::vector<std::vector<size_t>> widths((cells.size() + rows - 1) / rows);
    for (size_t i = 0; i < cells.size(); i++) {
        auto& w = widths[i / rows];
        if (w.size() < cells[i].size()) w.resize(cells[i].size(), 0);
        for (size_t p = 0; p < cells[i].size(); p++) w[p] = std::max(w[p], visible_width(cells[i][p]));
    }
    return widths;
}

static const size_t part_gap = 2, column_gap = 4;

std::vector<std::string> columns(const std::vector<std::vector<std::string>>& cells, size_t width, const std::string& indent) {
    if (cells.empty()) return {};
    size_t rows = cells.size();
    for (size_t cols = cells.size(); cols > 1 && width > 0; cols--) {
        size_t r = (cells.size() + cols - 1) / cols;
        size_t total = indent.size();
        auto widths = column_widths(cells, r);
        for (size_t c = 0; c < widths.size(); c++) {
            for (size_t p = 0; p < widths[c].size(); p++) total += widths[c][p] + (p ? part_gap : 0);
            if (c) total += column_gap;
        }
        if (total <= width) {
            rows = r;
            break;
        }
    }
    auto widths = column_widths(cells, rows);
    std::vector<std::string> out;
    for (size_t r = 0; r < rows; r++) {
        std::string line = indent;
        for (size_t c = 0; c < widths.size(); c++) {
            size_t i = c * rows + r;
            if (i >= cells.size()) break;
            if (c) line += std::string(column_gap, ' ');
            for (size_t p = 0; p < widths[c].size(); p++) {
                std::string part = p < cells[i].size() ? cells[i][p] : "";
                if (p) line += std::string(part_gap, ' ');
                line += part + std::string(widths[c][p] - visible_width(part), ' ');
            }
        }
        while (!line.empty() && line.back() == ' ') line.pop_back();
        out.push_back(line);
    }
    return out;
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
        while (!line.empty() && line.back() == ' ') line.pop_back();
        out.push_back(line);
    }
    return out;
}

}
