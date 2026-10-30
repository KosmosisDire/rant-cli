#include <optional>
#include <string>
#include <vector>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "library/library.hpp"
#include "library/uses.hpp"
#include "net/github.hpp"
#include "packages/packages.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "run/nodes.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace commands {

static const char* cli_repo = "KosmosisDire/rant-cli";

static std::string joined(const std::vector<std::string>& v) {
    std::string s;
    for (size_t i = 0; i < v.size(); i++) s += (i == 0 ? "" : i + 1 == v.size() ? " and " : ", ") + v[i];
    return s;
}

/* A package's move as the question lists it: explorer 0.0.19 to 0.0.20, from its source. */
static std::string shown(const packages::Update& u) {
    std::string from = u.source.repo.empty() ? config::to_utf8(u.source.path) : u.source.repo;
    return u.source.name + (u.had.empty() ? "" : " " + u.had) + (u.to.empty() ? "" : " to " + u.to) + ", from " + from;
}

/* Replaces this binary with its newest release and moves each installed package, or only
 * those named, to what its source offers now, after one question, never to an older release. */
static int update_programs(app::Context& ctx, packages::Sources& sources, const std::vector<std::string>& named) {
    bool all = named.empty();
    std::vector<std::string> current;
    net::Release cli;
    const net::Asset* a = nullptr;
    if (all) {
        cli = net::release(cli_repo);
        if (net::newer(cli.version, RANT_CLI_VERSION)) {
            std::string asset = "rant-" + cli.version + "-" + net::platform() + net::exe_suffix();
            a = cli.asset(asset);
            if (!a) throw app::Failure("the rant-cli " + cli.version + " release has no " + asset);
        } else {
            current.push_back("rant-cli " RANT_CLI_VERSION);
        }
    }
    std::vector<packages::Update> moves;
    /* a library's skill is installed as a package is, and moves with the packages */
    std::vector<std::pair<packages::Skill, net::Release>> skills;
    if (all) {
        std::vector<packages::Skill> had = packages::skills();
        std::vector<std::string> repos;
        for (auto& s : had) repos.push_back(s.repo);
        std::vector<std::optional<net::Release>> latest = net::latest(repos);
        for (size_t k = 0; k < had.size(); k++) {
            if (latest[k] && net::newer(latest[k]->version, had[k].version)) skills.push_back({ had[k], *latest[k] });
            else current.push_back("the skill of " + had[k].name + " " + had[k].version);
        }
    }
    int code = 0;
    for (auto& name : all ? packages::installed() : named) {
        try {
            std::optional<packages::Update> u = packages::check(ctx, sources, name);
            if (u && u->needed) moves.push_back(*u);
            else if (u) current.push_back(name + (u->had.empty() ? "" : " " + u->had));
        } catch (const app::Failure& e) {
            ctx.out.error(e.what());
            code = 1;
        }
    }
    if (!a && moves.empty() && skills.empty()) {
        if (!current.empty()) ctx.out.line(joined(current) + (current.size() > 1 ? " are" : " is") + " up to date");
        return code;
    }

    config::fs::path self = process::self_path();
    ctx.out.line("This updates:");
    if (a) ctx.out.line("  rant-cli " RANT_CLI_VERSION " to " + cli.version + ", over " + config::to_utf8(self));
    for (auto& m : moves) ctx.out.line("  " + shown(m));
    for (auto& [s, r] : skills) ctx.out.line("  the skill of " + s.name + " " + s.version + " to " + r.version + ", from " + s.repo);
    if (!ui::confirm("Update?", ctx.yes)) return 1;
    for (auto& [s, r] : skills) {
        try {
            packages::give_skill(ctx, s.name, s.repo, r, false);
        } catch (const app::Failure& e) {
            ctx.out.error(e.what());
            code = 1;
        }
    }

    for (auto& m : moves) {
        /* Windows will not replace a folder a node runs from, so its nodes stop meanwhile */
        config::fs::path folder = packages::dir() / m.source.name;
        std::vector<run::Paused> paused;
        if (!packages::is_link(folder)) paused = run::pause_from(folder);
        std::string stopped;
        for (auto& p : paused) stopped += (stopped.empty() ? "" : ", ") + p.rec.name;
        if (!paused.empty()) ctx.out.note("stopped " + stopped + " while " + m.source.name + " changes");
        try {
            packages::install(ctx, m.source, m.source.repo.empty() ? "" : m.to);
        } catch (const app::Failure& e) {
            ctx.out.error(e.what());
            code = 1;
        }
        if (paused.empty()) continue;
        std::vector<std::string> failed = run::resume(paused);
        for (auto& f : failed) ctx.out.warn("not started again, " + f);
        if (failed.empty()) ctx.out.note("started " + stopped + " again");
        else code = 1;
    }
    if (!a) return code;
    net::download(a->url, self, true);
    ctx.out.line("updated to rant-cli " + cli.version);
    /* the new binary holds new shell hooks, written again where setup wrote them before */
    config::fs::path hooks = util::rant_home() / "completions";
    for (auto [file, shell] : { std::pair<const char*, const char*>{ "rant.bash", "bash" }, { "rant.zsh", "zsh" }, { "rant.ps1", "powershell" } })
        if (config::fs::exists(hooks / file) &&
            process::run({ { config::to_utf8(self), "setup", shell }, config::fs::temp_directory_path(), {} }) != 0)
            ctx.out.warn(std::string("tab completion for ") + shell + " was not updated, run `rant setup " + shell + "`");
    return code;
}

/* The CLI and the installed packages first, then the libraries the packages of the workspace
 * here use, each after its own question. Named packages and libraries alone go without the rest. */
static int run(app::Context& ctx) {
    packages::Sources sources;
    std::vector<std::string> programs;
    std::vector<library::Library> libs;
    for (auto& word : ctx.args.words) {
        std::optional<packages::Source> s = packages::record(word) ? std::nullopt : sources.find(word);
        if (s && s->library) libs.push_back(library::find(ctx, sources, word));
        else programs.push_back(word);
    }
    bool all = ctx.args.words.empty();
    int code = all || !programs.empty() ? update_programs(ctx, sources, programs) : 0;
    if (!all && libs.empty()) return code;
    const config::Workspace* ws = ctx.workspace();
    if (!ws) {
        if (!all) throw app::Failure("rant update moves a library through a workspace, and none is here");
        return code;
    }
    if (all) libs = library::all(sources);
    std::vector<library::Use> uses = library::under(ws->root, libs);
    if (uses.empty()) {
        if (!all) ctx.out.note("nothing in this workspace uses " + libs[0].name + (libs.size() > 1 ? " or the others named" : ""));
        return code;
    }
    if (all || !programs.empty()) ctx.out.line();
    return library::update(ctx, uses, libs) ? code : 1;
}

static complete::Candidates complete_words(complete::Request&) {
    std::vector<std::string> names = packages::installed();
    for (auto& s : packages::cached())
        if (s.library) names.push_back(s.name);
    return { names };
}

app::Command update() {
    app::Command c{ "update", "[package|library...]",
                    "update rant-cli and the installed packages, then the libraries this workspace's packages use. "
                    "With names, only those",
                    app::Section::Setup, {}, run };
    c.complete = complete_words;
    return c;
}

}
