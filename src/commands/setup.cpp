#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "complete/shells.hpp"

namespace commands {

static std::string shell_names() {
    std::string s;
    for (auto& sh : complete::shells()) s += (s.empty() ? "" : ", ") + std::string(sh.name);
    return s;
}

static const complete::Shell& named(const std::string& name) {
    const complete::Shell* sh = complete::find_shell(name);
    if (!sh) throw app::UsageError("unknown shell `" + name + "`, rant knows " + shell_names());
    return *sh;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (ctx.args.has("print")) {
        if (w.size() != 1) throw app::UsageError("--print takes one shell, one of " + shell_names());
        ctx.out.line(std::string(named(w[0]).hook));
        return 0;
    }
    std::vector<const complete::Shell*> chosen;
    for (auto& name : w) chosen.push_back(&named(name));
    if (w.empty()) chosen = complete::detected();
    if (chosen.empty()) throw app::Failure("found no shell to set up, name one of " + shell_names());

    bool failed = false;
    for (auto* sh : chosen) {
        try {
            ctx.out.line(std::string(sh->name) + ": tab completion " + complete::install(*sh));
        } catch (const app::Failure& e) {
            ctx.out.warn(std::string(sh->name) + ": " + e.what());
            failed = true;
        }
    }
    ctx.out.note("open a new shell to use it");
    return failed ? 1 : 0;
}

static complete::Candidates complete_words(complete::Request&) {
    std::vector<std::string> out;
    for (auto& sh : complete::shells()) out.emplace_back(sh.name);
    return { out };
}

app::Command setup() {
    app::Command c{ "setup", "[shell...]", "set up tab completion for your shells, or the named ones",
                    app::Section::Setup,
                    { { "print", 0, "", "print the hook for one shell instead of installing it" } }, run };
    c.complete = complete_words;
    return c;
}

}
