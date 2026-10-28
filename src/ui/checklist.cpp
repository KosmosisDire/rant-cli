#include "ui/checklist.hpp"

#include <algorithm>
#include <cstdio>

#include "ui/live.hpp"
#include "ui/terminal.hpp"

namespace ui {

Checklist::Checklist(std::string title, std::vector<Choice> choices) : title_(std::move(title)), choices_(std::move(choices)) {}

bool Checklist::press(Key k) {
    if (ended_) return false;
    switch (k) {
    case Key::Up:
        at_ = at_ == 0 ? choices_.size() - 1 : at_ - 1;
        break;
    case Key::Down:
        at_ = at_ + 1 == choices_.size() ? 0 : at_ + 1;
        break;
    case Key::Toggle:
        if (!choices_.empty()) choices_[at_].checked = !choices_[at_].checked;
        break;
    case Key::All: {    /* all when any is off, else none */
        bool all = std::all_of(choices_.begin(), choices_.end(), [](const Choice& c) { return c.checked; });
        for (auto& c : choices_) c.checked = !all;
        break;
    }
    case Key::Enter: ended_ = confirmed_ = true; break;
    case Key::Quit: ended_ = true; break;
    case Key::Other: break;
    }
    return !ended_;
}

std::vector<size_t> Checklist::ticked() const {
    std::vector<size_t> out;
    for (size_t i = 0; i < choices_.size(); i++)
        if (choices_[i].checked) out.push_back(i);
    return out;
}

std::vector<std::string> Checklist::lines(const Output& out) const {
    std::string mark = out.paint_err(Style::Accent, "?") + " " + out.paint_err(Style::Bold, title_);
    if (ended_) {
        std::string picked;
        for (size_t i : ticked()) picked += (picked.empty() ? "" : ", ") + choices_[i].label;
        if (!confirmed_) picked = "cancelled";
        else if (picked.empty()) picked = "none";
        return { mark + "  " + out.paint_err(Style::Dim, picked) };
    }
    std::vector<std::string> lines{ mark, out.paint_err(Style::Faint, "  up and down move, space ticks, a ticks all, enter confirms, q cancels") };
    size_t width = 0;
    for (auto& c : choices_) width = std::max(width, c.label.size());
    for (size_t i = 0; i < choices_.size(); i++) {
        const Choice& c = choices_[i];
        bool here = i == at_;
        std::string label = c.label + std::string(width - c.label.size(), ' ');
        lines.push_back((here ? out.paint_err(Style::Accent, "> ") : "  ") + (c.checked ? "[x] " : "[ ] ") +
                        (here ? out.paint_err(Style::Accent, label) : label) + "  " + out.paint_err(Style::Faint, c.note));
    }
    return lines;
}

std::optional<std::vector<size_t>> pick(const Output& out, std::string title, std::vector<Choice> choices) {
    std::fflush(stdout);
    if (choices.empty() || !enable_vt(stderr)) return std::nullopt;
    KeyReader keys;
    if (!keys.ok()) return std::nullopt;
    Checklist list(std::move(title), std::move(choices));
    {
        LiveView view(stderr);
        view.draw(list.lines(out));
        while (list.press(keys.next())) view.draw(list.lines(out));
        view.draw(list.lines(out));
    }
    if (!list.confirmed()) return std::nullopt;
    return list.ticked();
}

}
