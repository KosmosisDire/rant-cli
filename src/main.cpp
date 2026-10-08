#include <exception>

#include "app/command.hpp"
#include "app/failure.hpp"
#include "ui/terminal.hpp"

int main(int argc, char** argv) {
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
