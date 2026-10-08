#include "ui/live.hpp"

#include <cstdio>

#include "ui/terminal.hpp"

namespace ui {

static void put(const std::string& s) {
    std::fwrite(s.data(), 1, s.size(), stdout);
}

LiveView::LiveView() { put("\x1b[?25l"); }

LiveView::~LiveView() {
    put("\x1b[?25h");
    std::fflush(stdout);
}

std::string clip(const std::string& text, size_t cols) {
    std::string out;
    size_t visible = 0;
    bool cut = false;
    for (size_t i = 0; i < text.size(); i++) {
        if (text[i] == '\x1b') {    /* a color code costs no width and is always kept */
            size_t end = text.find('m', i);
            if (end == std::string::npos) break;
            out.append(text, i, end - i + 1);
            i = end;
            continue;
        }
        bool lead = ((unsigned char)text[i] & 0xC0) != 0x80;
        if (lead && visible == cols) {
            cut = true;
            continue;
        }
        if (cut) continue;
        if (lead) visible++;
        out += text[i];
    }
    return out + (cut ? "\x1b[0m" : "");
}

void LiveView::draw(const std::vector<std::string>& lines) {
    Size size = terminal_size();
    size_t cols = size.cols > 1 ? (size_t)size.cols - 1 : 200;
    size_t room = size.rows > 2 ? (size_t)size.rows - 1 : lines.size();
    std::string frame;
    if (drawn_) frame += "\x1b[" + std::to_string(drawn_) + "F";    /* back to the first line */
    frame += "\x1b[J";
    size_t shown = lines.size() <= room ? lines.size() : room - 1;
    for (size_t i = 0; i < shown; i++) frame += clip(lines[i], cols) + "\n";
    if (shown < lines.size()) frame += clip("... " + std::to_string(lines.size() - shown) + " more lines", cols) + "\n";
    drawn_ = shown + (shown < lines.size() ? 1 : 0);
    put(frame);
    std::fflush(stdout);
}

}
