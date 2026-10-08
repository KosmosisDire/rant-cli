#pragma once

#include <string>
#include <string_view>

namespace ui {

enum class Style { Plain, Bold, Dim, Red, Green, Yellow, Cyan };

/* Everything the CLI prints goes through here, so color follows one rule: only when the
 * stream is a terminal and NO_COLOR is unset. Data goes to stdout, the rest to stderr. */
class Output {
public:
    Output();

    std::string paint(Style s, std::string_view text) const;
    std::string paint_err(Style s, std::string_view text) const;

    void line(std::string_view text = {}) const;   /* stdout */
    void error(std::string_view text) const;       /* stderr, "error: text" */
    void warn(std::string_view text) const;        /* stderr, "warning: text" */
    void note(std::string_view text) const;        /* stderr, plain */

private:
    bool color_out_;
    bool color_err_;
};

}
