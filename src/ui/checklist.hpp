#pragma once

#include <optional>
#include <string>
#include <vector>

#include "ui/keys.hpp"
#include "ui/output.hpp"

namespace ui {

/* One row of a checklist: a name, and faint words after it such as a description. */
struct Choice {
    std::string label;
    std::string note;
    bool        checked = false;
};

/* A checklist's state, kept apart from the terminal so a test can drive it. */
class Checklist {
public:
    Checklist(std::string title, std::vector<Choice> choices);

    /* Moves, ticks or ends. False once Enter or Quit ended it. */
    bool press(Key k);

    bool done() const { return ended_; }
    bool confirmed() const { return confirmed_; }
    std::vector<size_t> ticked() const;

    /* The frame to draw: the title, a line of keys, then a row per choice, the current one
     * marked in the accent. Once ended, one line with what was picked. */
    std::vector<std::string> lines(const Output& out) const;

private:
    std::string         title_;
    std::vector<Choice> choices_;
    size_t              at_ = 0;
    bool                ended_ = false;
    bool                confirmed_ = false;
};

/* Asks on stderr with a checklist. The ticked indexes, or nullopt when it was given up or
 * stdin is no terminal. */
std::optional<std::vector<size_t>> pick(const Output& out, std::string title, std::vector<Choice> choices);

}
