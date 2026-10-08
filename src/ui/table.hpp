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

/* Cells laid out down then across in as many columns as fit width, the parts of each cell
 * lined up within its column, as ls does. One column when width is 0. */
std::vector<std::string> columns(const std::vector<std::vector<std::string>>& cells, size_t width,
                                 const std::string& indent = "  ");

/* Several lists of cells on one grid, every column as wide as the widest cell, so a column
 * lines up from one list to the next. Each list fills down then across. */
std::vector<std::vector<std::string>> columns(const std::vector<std::vector<std::vector<std::string>>>& lists,
                                              size_t width, const std::string& indent);

}
