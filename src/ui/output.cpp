#include "ui/output.hpp"

#include <cstdio>
#include <cstdlib>

#include "ui/terminal.hpp"

namespace ui {

static bool no_color() {
    const char* v = std::getenv("NO_COLOR");
    return v && *v;
}

Output::Output()
    : color_out_(!no_color() && enable_vt(stdout)),
      color_err_(!no_color() && enable_vt(stderr)) {}

static std::string wrap(bool on, Style s, std::string_view text) {
    if (!on || s == Style::Plain) return std::string(text);
    const char* code = "";
    switch (s) {
    case Style::Bold:    code = "1"; break;
    case Style::Dim:     code = "38;2;154;154;154"; break;
    case Style::Faint:   code = "38;2;100;100;100"; break;
    case Style::Accent:  code = "38;2;220;56;83"; break;
    case Style::Green:   code = "38;2;55;136;95"; break;
    case Style::GreenHi: code = "38;2;92;166;125"; break;
    case Style::Amber:   code = "38;2;207;154;58"; break;
    case Style::Red:     code = "38;2;210;87;75"; break;
    case Style::Plain:   break;
    }
    return std::string("\x1b[") + code + "m" + std::string(text) + "\x1b[0m";
}

std::string Output::paint(Style s, std::string_view text) const { return wrap(color_out_, s, text); }
std::string Output::paint_err(Style s, std::string_view text) const { return wrap(color_err_, s, text); }

void Output::line(std::string_view text) const {
    std::fwrite(text.data(), 1, text.size(), stdout);
    std::fputc('\n', stdout);
}

void Output::error(std::string_view text) const {
    std::fflush(stdout);
    std::fprintf(stderr, "%s %.*s\n", paint_err(Style::Red, "error:").c_str(), (int)text.size(), text.data());
}

void Output::warn(std::string_view text) const {
    std::fflush(stdout);
    std::fprintf(stderr, "%s %.*s\n", paint_err(Style::Amber, "warning:").c_str(), (int)text.size(), text.data());
}

void Output::note(std::string_view text) const {
    std::fflush(stdout);
    std::fprintf(stderr, "%.*s\n", (int)text.size(), text.data());
}

}
