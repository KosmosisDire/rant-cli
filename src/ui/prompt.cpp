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
    if (!std::getline(std::cin, answer)) return false;
    return answer == "y" || answer == "Y" || answer == "yes";
}

}
