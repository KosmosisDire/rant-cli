#pragma once

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace ui {

/* Values as CSV rows, one column per leaf: pose.x, pts[0].y, or value for a bare value.
 * The first row fixes the columns, later rows fill them in that order and leave out a
 * column the first row did not have. Numbers keep their full precision. */
class Csv {
public:
    /* The header line, then the row, on the first call. The row alone after that. */
    std::string rows(const nlohmann::ordered_json& v);

private:
    std::vector<std::string> columns_;
};

}
