#include "commands/commands.hpp"

namespace commands {

const std::vector<app::Command>& all() {
    static const std::vector<app::Command> list = {
        init(),
        build(),
        start(),
        stop(),
        ls(),
        info(),
        sub(),
        pub(),
        get(),
        set(),
        call(),
        setup(),
        lib(),
    };
    return list;
}

}
