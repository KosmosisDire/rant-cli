#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "packages/packages.hpp"

namespace commands {

static int run(app::Context& ctx) {
    auto& names = ctx.args.words;
    if (names.empty()) throw app::UsageError("remove needs an installed package, `rant ls packages` lists them");
    int code = 0;
    for (auto& name : names) {
        try {
            packages::remove(ctx, name);
        } catch (const app::Failure& e) {
            if (names.size() == 1) throw;
            ctx.out.error(e.what());
            code = 1;
        }
    }
    return code;
}

static complete::Candidates complete_words(complete::Request&) { return { packages::installed() }; }

app::Command remove() {
    app::Command c{ "remove", "<package>...", "remove installed packages, a linked build folder stays as it is", app::Section::Setup, {}, run };
    c.complete = complete_words;
    return c;
}

}
