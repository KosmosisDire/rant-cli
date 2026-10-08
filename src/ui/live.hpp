#pragma once

#include <string>
#include <vector>

namespace ui {

/* A block of lines redrawn in place on a terminal, each frame replacing the last. Lines
 * are cut to the terminal's width and the block to its height, with a last line saying how
 * much did not fit, so the cursor always lands back on the first line. The cursor hides
 * while the view lives, and the last frame stays on screen after it. */
class LiveView {
public:
    LiveView();
    ~LiveView();
    LiveView(const LiveView&) = delete;
    LiveView& operator=(const LiveView&) = delete;

    void draw(const std::vector<std::string>& lines);

private:
    size_t drawn_ = 0;
};

/* text cut to cols visible characters, color codes kept whole and closed. */
std::string clip(const std::string& text, size_t cols);

}
