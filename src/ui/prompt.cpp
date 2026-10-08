#include "ui/prompt.hpp"

#include <cstdio>
#include <iostream>
#include <string>

#include "ui/terminal.hpp"

namespace ui {

bool confirm(std::string_view question, bool assume_yes) {
    std::fflush(stdout);
    if (assume_yes || !is_terminal(stdin)) {
        std::fprintf(stderr, "%.*s [y/N] y%s\n", (int)question.size(), question.data(),
                     assume_yes ? "" : " (stdin is not a terminal)");
        return true;
    }
    std::fprintf(stderr, "%.*s [y/N] ", (int)question.size(), question.data());
    std::fflush(stderr);
    std::string answer;
    if (!std::getline(std::cin, answer)) {
        std::fputc('\n', stderr);    /* no answer came, so the next line starts clean */
        return false;
    }
    return answer == "y" || answer == "Y" || answer == "yes";
}

std::optional<std::string> ask(std::string_view question) {
    std::fflush(stdout);
    if (!is_terminal(stdin)) return std::nullopt;
    std::fprintf(stderr, "%.*s ", (int)question.size(), question.data());
    std::fflush(stderr);
    std::string answer;
    if (!std::getline(std::cin, answer)) {
        std::fputc('\n', stderr);
        return std::nullopt;
    }
    return answer;
}

}
