#include <cstdlib>
#include <exception>

#include "app/command.hpp"
#include "app/failure.hpp"
#include "process/supervisor.hpp"
#include "ui/terminal.hpp"

int main(int argc, char** argv) {
    if (auto code = process::helper_main(argc, argv)) return *code;
    /* A launcher name in our own environment was meant for a node, never for the CLI's own
     * observer nodes. Nodes we start get theirs set explicitly. */
#ifdef _WIN32
    _putenv("RANT_NODE_NAME=");
#else
    unsetenv("RANT_NODE_NAME");
#endif
    app::Context ctx;
    try {
        return app::run(ctx, ui::utf8_args(argc, argv));
    } catch (const app::UsageError& e) {
        ctx.out.error(e.what());
        return e.code();
    } catch (const app::Failure& e) {
        if (*e.what()) ctx.out.error(e.what());
        return e.code();
    } catch (const std::exception& e) {
        ctx.out.error(e.what());
        return 1;
    }
}
