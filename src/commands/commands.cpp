#include "commands/commands.hpp"

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

}
