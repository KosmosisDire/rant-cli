#include "app/failure.hpp"
#include "commands/commands.hpp"

namespace commands {

static int run(app::Context& ctx) {
    if (ctx.args.words.size() > 1) throw app::UsageError("init takes at most one directory");
    auto dir = ctx.args.words.empty() ? ctx.cwd() : ctx.cwd() / config::from_utf8(ctx.args.words[0]);
    auto opened = config::init(dir);
    for (auto& d : opened.diagnostics) ctx.out.error(d.str());
    if (!opened.workspace) throw app::Failure("");
    ctx.out.line("created " + config::to_utf8(opened.workspace->root / "rant.hcl"));
    return 0;
}

app::Command init() {
    return { "init", "[dir]", "make this directory a workspace by writing rant.hcl",
             app::Section::Workspace, {}, run };
}

}
