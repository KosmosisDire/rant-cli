#include "commands/commands.hpp"

#include <algorithm>

namespace commands {

const std::vector<app::Command>& all() {
    static const std::vector<app::Command> list = {
        init(),
        new_(),
        build(),
        run_(),
        stop(),
        restart(),
        enable(),
        disable(),
        log(),
        ls(),
        info(),
        sub(),
        pub(),
        call(),
        setup(),
        lib(),
        install(),
        remove(),
        update(),
        uninstall(),
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
