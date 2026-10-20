#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "packages/packages.hpp"

namespace commands {

/* Each package from where sources.rant or a database names it, or from --from. With several,
 * one that fails leaves the rest to go on. */
static int run(app::Context& ctx) {
    auto& names = ctx.args.words;
    if (names.empty()) throw app::UsageError("install needs a package, `rant ls packages --available` lists them");
    std::optional<std::string> version = ctx.args.get("version"), from = ctx.args.get("from");
    if ((version || from) && names.size() > 1) throw app::UsageError(std::string(from ? "--from" : "--version") + " takes one package");
    if (version && from) throw app::UsageError("--version picks a release, and --from has none");
    std::optional<packages::Sources> sources;
    int code = 0;
    for (auto& name : names) {
        try {
            if (!config::is_package_name(name))
                throw app::UsageError("`" + name + "` is no package name, which is lowercase letters, digits and _");
            packages::Source s;
            if (from) {
                s = { name, "", ctx.cwd() / config::from_utf8(*from), "--from" };
            } else {
                if (!sources) sources.emplace();
                std::optional<packages::Source> found = sources->find(name);
                if (!found) throw app::Failure("no package `" + name + "` in sources.rant or the package databases, `rant ls packages --available` lists them");
                s = *found;
            }
            if (version && s.repo.empty())
                throw app::UsageError("--version picks a release, and " + name + " comes from " + config::to_utf8(s.path) + " in sources.rant");
            packages::install(ctx, s, version.value_or(""));
        } catch (const app::Failure& e) {
            if (names.size() == 1) throw;
            ctx.out.error(e.what());
            code = 1;
        }
    }
    return code;
}

static complete::Candidates complete_words(complete::Request&) { return { packages::cached_names() }; }

app::Command install() {
    app::Command c{ "install", "<package>...",
                    "install packages, or another release of one, from ~/.rant/sources.rant and the package databases, "
                    "`rant ls sources` shows where it looks",
                    app::Section::Setup,
                    { { "version", 0, "V", "with one package: this release instead of the newest" },
                      { "from", 0, "PATH", "with one package: a local build folder, linked, or a .tar.gz archive, instead of its source" },
                      { "editor", 0, "PROGRAM", "for a VS Code extension: the editor's command line program, default `code`, such as `cursor`" },
                      { "profile", 0, "NAME", "for a VS Code extension: the editor profile, default the one this VS Code terminal's folder opens in, else asked" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
