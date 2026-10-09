#include "commands/commands.hpp"

#include <algorithm>

namespace commands {

const std::vector<app::Command>& all() {
    static const std::vector<app::Command> list = {
        init(),
        new_(),
        build(),
        start(),
        stop(),
        restart(),
        ls(),
        info(),
        sub(),
        pub(),
        get(),
        set(),
        call(),
        setup(),
        lib(),
        explore(),
    };
    return list;
}

const app::Command* find(std::string_view word) {
    for (auto& c : all())
        if (c.name == word || std::find(c.aliases.begin(), c.aliases.end(), word) != c.aliases.end()) return &c;
    return nullptr;
}

}
