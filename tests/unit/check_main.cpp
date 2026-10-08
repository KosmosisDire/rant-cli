#include <cstring>

#include "check.hpp"
#include "process/supervisor.hpp"

int main(int argc, char** argv) {
    if (auto code = process::helper_main(argc, argv)) return *code;
    int run = 0;
    for (auto& c : check::cases()) {
        bool wanted = argc < 2;
        for (int i = 1; i < argc; i++) wanted |= std::strcmp(argv[i], c.name) == 0;
        if (!wanted) continue;
        int before = check::failures();
        c.fn();
        run++;
        std::printf("%s %s\n", check::failures() == before ? "ok  " : "FAIL", c.name);
    }
    if (run == 0) {
        std::fprintf(stderr, "no test case matched\n");
        return 1;
    }
    return check::failures() ? 1 : 0;
}
