#include <algorithm>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/enable.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "net/github.hpp"
#include "packages/packages.hpp"
#include "packages/vsix.hpp"
#include "ui/checklist.hpp"
#include "ui/terminal.hpp"

namespace commands {

/* Each package from where sources.rant or a database names it, a repo by its link, or one
 * from --from. With several, one that fails leaves the rest to go on. Returns the names that
 * installed. */
static std::vector<std::string> install_all(app::Context& ctx, const std::vector<std::string>& words, int& code) {
    std::optional<std::string> version = ctx.args.get("version"), from = ctx.args.get("from");
    if ((version || from) && words.size() > 1) throw app::UsageError(std::string(from ? "--from" : "--version") + " takes one package");
    if (version && from) throw app::UsageError("--version picks a release, and --from has none");
    std::optional<packages::Sources> sources;
    std::vector<std::string> done;
    for (auto& word : words) {
        try {
            std::optional<std::string> repo = config::is_package_name(word) ? std::nullopt : config::repo_of(word);
            if (!config::is_package_name(word) && !repo)
                throw app::UsageError("`" + word + "` is no package name, which is lowercase letters, digits and _, and no GitHub repo");
            if (from && repo) throw app::UsageError("--from takes the name of the package it holds, not a repo");
            packages::Source s;
            if (from) {
                s = { word, "", ctx.cwd() / config::from_utf8(*from), "--from" };
            } else if (repo) {
                s = packages::from_repo(*repo);
            } else {
                if (!sources) sources.emplace();
                std::optional<packages::Source> found = sources->find(word);
                if (!found) {
                    for (auto& p : sources->problems()) ctx.out.warn(p);
                    throw app::Failure("no package `" + word + "` in sources.rant or the package databases, `rant ls packages --available` lists them");
                }
                s = *found;
            }
            if (version && s.repo.empty())
                throw app::UsageError("--version picks a release, and " + word + " comes from " + config::to_utf8(s.path) + " in sources.rant");
            packages::install(ctx, s, version.value_or(""));
            done.push_back(s.name);
        } catch (const app::Failure& e) {
            if (words.size() == 1) throw;
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

/* Whether each offer is a VS Code extension its latest release carries, all false when that
 * cannot be told. */
static std::vector<bool> extensions(app::Context& ctx, const std::vector<packages::Offer>& offers) {
    std::vector<bool> out(offers.size(), false);
    std::vector<std::string> repos;
    std::vector<size_t> at;
    for (size_t i = 0; i < offers.size(); i++)
        if (!offers[i].source.repo.empty() && !offers[i].record) {
            repos.push_back(offers[i].source.repo);
            at.push_back(i);
        }
    try {
        std::vector<std::optional<net::Release>> found = net::latest(repos);
        for (size_t k = 0; k < at.size(); k++)
            out[at[k]] = found[k] && packages::carried(*found[k], offers[at[k]].source.name, net::platform()).extension;
    } catch (const app::Failure& e) {
        ctx.out.warn(e.what());
    }
    return out;
}

/* With no names on a terminal: a checklist of every package there is, those not installed
 * ticked but an editor extension with no editor found, then their groups to run at every boot. */
static int pick(app::Context& ctx) {
    if (ctx.yes || !ui::is_terminal(stdin) || ctx.args.has("version") || ctx.args.has("from"))
        throw app::UsageError("install needs a package, `rant ls packages --available` lists them");
    ctx.out.note("looking up the packages");
    packages::Sources sources;
    std::vector<packages::Offer> offers = sources.offers();
    offers.erase(std::remove_if(offers.begin(), offers.end(), [](const packages::Offer& o) { return o.source.library.has_value(); }), offers.end());
    for (auto& p : sources.problems()) ctx.out.warn(p);
    std::vector<bool> skipped = packages::vsix::editor_found(ctx) ? std::vector<bool>(offers.size(), false) : extensions(ctx, offers);
    std::vector<ui::Choice> choices;
    for (size_t i = 0; i < offers.size(); i++) {
        auto& o = offers[i];
        std::string state;
        if (o.record) state = o.record->version.empty() ? "installed, " : "installed " + o.record->version + ", ";
        else if (skipped[i]) state = "no editor found, ";
        choices.push_back({ o.source.name, state + o.description, !o.record && !skipped[i] });
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

static complete::Candidates complete_words(complete::Request&) {
    std::vector<std::string> names;
    for (auto& s : packages::cached())
        if (!s.library) names.push_back(s.name);
    return { names };
}

app::Command install() {
    app::Command c{ "install", "[package|repo...]",
                    "install packages, or another release of one, by name from ~/.rant/sources.rant and the package databases, "
                    "`rant ls sources` shows where it looks, or from any GitHub repo as owner/name or its link. "
                    "With none named, pick them from a list",
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
