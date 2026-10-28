#include <algorithm>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/enable.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "packages/packages.hpp"
#include "ui/checklist.hpp"
#include "ui/terminal.hpp"

namespace commands {

/* Each package from where sources.rant or a database names it, or from --from. With several,
 * one that fails leaves the rest to go on. Returns the names that installed. */
static std::vector<std::string> install_all(app::Context& ctx, const std::vector<std::string>& names, int& code) {
    std::optional<std::string> version = ctx.args.get("version"), from = ctx.args.get("from");
    if ((version || from) && names.size() > 1) throw app::UsageError(std::string(from ? "--from" : "--version") + " takes one package");
    if (version && from) throw app::UsageError("--version picks a release, and --from has none");
    std::optional<packages::Sources> sources;
    std::vector<std::string> done;
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
            done.push_back(name);
        } catch (const app::Failure& e) {
            if (names.size() == 1) throw;
            ctx.out.error(e.what());
            code = 1;
        }
    }
    return done;
}

/* The groups of the packages just installed, picked to run at every boot. */
static int offer_boot(app::Context& ctx, const std::vector<std::string>& installed) {
    config::Opened opened = config::open(ctx.cwd(), true);
    if (!opened.workspace) return 0;
    std::vector<std::string> names;
    std::vector<ui::Choice> choices;
    for (auto& g : opened.workspace->groups) {
        std::string package = g.name.substr(0, g.name.find('/'));
        if (g.name.find('/') == std::string::npos || std::find(installed.begin(), installed.end(), package) == installed.end())
            continue;
        names.push_back(g.short_name);
        choices.push_back({ g.short_name, config::describe_group(ctx.cwd(), g.name).description });
    }
    if (choices.empty()) return 0;
    std::optional<std::vector<size_t>> ticked = ui::pick(ctx.out, "Run at every boot", choices);
    if (!ticked) return 0;
    int code = 0;
    for (size_t i : *ticked) {
        try {
            if (enable_named(ctx, { names[i] }) != 0) code = 1;
        } catch (const app::Failure& e) {
            ctx.out.error(e.what());
            code = 1;
        }
    }
    return code;
}

/* With no names on a terminal: a checklist of every package there is, then one of their
 * groups to run at every boot. */
static int pick(app::Context& ctx) {
    if (ctx.yes || !ui::is_terminal(stdin) || ctx.args.has("version") || ctx.args.has("from"))
        throw app::UsageError("install needs a package, `rant ls packages --available` lists them");
    ctx.out.note("looking up the packages");
    packages::Sources sources;
    std::vector<packages::Offer> offers = packages::offers(sources);
    std::vector<ui::Choice> choices;
    for (auto& o : offers) {
        std::string state;
        if (o.record) state = o.record->version.empty() ? "installed, " : "installed " + o.record->version + ", ";
        choices.push_back({ o.source.name, state + (o.problem.empty() ? o.description : o.problem) });
    }
    std::optional<std::vector<size_t>> ticked = ui::pick(ctx.out, "Packages to install", choices);
    if (!ticked || ticked->empty()) return 0;
    std::vector<std::string> names;
    for (size_t i : *ticked) names.push_back(offers[i].source.name);
    int code = 0;
    std::vector<std::string> done = install_all(ctx, names, code);
    if (offer_boot(ctx, done) != 0) code = 1;
    return code;
}

static int run(app::Context& ctx) {
    if (ctx.args.words.empty()) return pick(ctx);
    int code = 0;
    install_all(ctx, ctx.args.words, code);
    return code;
}

static complete::Candidates complete_words(complete::Request&) { return { packages::cached_names() }; }

app::Command install() {
    app::Command c{ "install", "[package...]",
                    "install packages, or another release of one, from ~/.rant/sources.rant and the package databases, "
                    "`rant ls sources` shows where it looks. With none named, pick them from a list",
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
