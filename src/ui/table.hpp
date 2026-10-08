#pragma once

#include <string>
#include <vector>

namespace ui {

/* Rows of cells printed in aligned columns. Color codes in a cell do not count toward
 * its width. */
class Table {
public:
    void row(std::vector<std::string> cells) { rows_.push_back(std::move(cells)); }
    bool empty() const { return rows_.empty(); }

    /* One line per row, indented, columns separated by two spaces, no trailing space. */
    std::vector<std::string> lines(const std::string& indent = "  ") const;

private:
    std::vector<std::vector<std::string>> rows_;
};

size_t visible_width(const std::string& s);

}
