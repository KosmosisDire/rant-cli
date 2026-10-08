#include "commands/commands.hpp"

namespace commands {

const std::vector<app::Command>& all() {
    static const std::vector<app::Command> list = {
        init(),
        ls(),
        info(),
        sub(),
    };
    return list;
}

}
