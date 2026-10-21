#include <optional>
#include <string>
#include <vector>

#include "agents/skill.hpp"
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
 * those named, to what its source offers now, after one question, never to an older release.
 * The skill moves with the CLI. */
static int update_programs(app::Context& ctx, const std::vector<std::string>& named) {
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
    packages::Sources sources;
    std::vector<packages::Update> moves;
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
    if (!a && moves.empty()) {
        if (!current.empty()) ctx.out.line(joined(current) + (current.size() > 1 ? " are" : " is") + " up to date");
        return code;
    }

    config::fs::path self = process::self_path();
    std::string names;
    if (a)
        for (auto& agent : agents::detected())
            if (agents::installed(agent.skills)) names += (names.empty() ? "" : ", ") + agent.name;
    ctx.out.line("This updates:");
    if (a) ctx.out.line("  rant-cli " RANT_CLI_VERSION " to " + cli.version + ", over " + config::to_utf8(self));
    for (auto& m : moves) ctx.out.line("  " + shown(m));
    if (!names.empty()) ctx.out.line("  the rant skill for " + names);
    if (!ui::confirm("Update?", ctx.yes)) return 1;

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
    /* the new binary holds the new skill */
    if (!names.empty() && process::run({ { config::to_utf8(self), "setup", "skill" }, config::fs::temp_directory_path(), {} }) != 0)
        ctx.out.warn("the rant skill was not updated, run `rant setup skill`");
    /* and new shell hooks, written again where setup wrote them before */
    config::fs::path hooks = util::rant_home() / "completions";
    for (auto [file, shell] : { std::pair<const char*, const char*>{ "rant.bash", "bash" }, { "rant.zsh", "zsh" }, { "rant.ps1", "powershell" } })
        if (config::fs::exists(hooks / file) &&
            process::run({ { config::to_utf8(self), "setup", shell }, config::fs::temp_directory_path(), {} }) != 0)
            ctx.out.warn(std::string("tab completion for ") + shell + " was not updated, run `rant setup " + shell + "`");
    return code;
}

/* The CLI and the installed packages first, then every package of the workspace
 * here that uses Rant, each after its own question. Named packages alone go without the rest. */
static int run(app::Context& ctx) {
    int code = update_programs(ctx, ctx.args.words);
    if (!ctx.args.words.empty()) return code;
    const config::Workspace* ws = ctx.workspace();
    if (!ws || library::under(ws->root).empty()) return code;
    ctx.out.line();
    return library::update(ctx, ws->root, library::release("")) ? code : 1;
}

static complete::Candidates complete_words(complete::Request&) { return { packages::installed() }; }

app::Command update() {
    app::Command c{ "update", "[package...]",
                    "update rant-cli and the installed packages, then the Rant library of this workspace's packages. "
                    "With packages named, only those",
                    app::Section::Setup, {}, run };
    c.complete = complete_words;
    return c;
}

}
