#include <cstdlib>
#include <optional>
#include <string>
#include <vector>

#include "agents/skill.hpp"
#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/explore.hpp"
#include "config/config.hpp"
#include "library/library.hpp"
#include "library/uses.hpp"
#include "net/github.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "ui/prompt.hpp"

namespace commands {

static const char* cli_repo = "KosmosisDire/rant-cli";

/* The numbers of a dotted version, 0.0.12 as {0, 0, 12}, compared number by number. */
static std::vector<long> numbers(const std::string& v) {
    std::vector<long> out;
    size_t at = 0;
    while (at < v.size()) {
        size_t dot = v.find('.', at);
        out.push_back(std::strtol(v.substr(at, dot - at).c_str(), nullptr, 10));
        if (dot == std::string::npos) break;
        at = dot + 1;
    }
    return out;
}

/* Replaces this binary and the explorer rant installed with their newest releases, after
 * one question. The skill moves with the CLI, both only where someone installed them. */
static int update_programs(app::Context& ctx) {
    net::Release cli = net::release(cli_repo);
    const net::Asset* a = nullptr;
    if (numbers(cli.version) > numbers(RANT_CLI_VERSION)) {
        std::string asset = "rant-" + cli.version + "-" + net::platform();
        a = cli.asset(asset);
        if (!a) throw app::Failure("the rant-cli " + cli.version + " release has no " + asset);
    }
    std::optional<net::Release> ex;
    std::string ex_had = explorer::installed_version();
    if (config::fs::is_regular_file(explorer::installed())) {
        ex = explorer::latest();
        if (ex->version == ex_had) ex.reset();
    }
    if (!a && !ex) {
        bool has = config::fs::is_regular_file(explorer::installed());
        ctx.out.line("rant-cli " RANT_CLI_VERSION + std::string(has ? " and the explorer " + ex_had + " are" : " is") + " up to date");
        return 0;
    }

    config::fs::path self = process::self_path();
    std::string names;
    if (a)
        for (auto& agent : agents::detected())
            if (agents::installed(agent.skills)) names += (names.empty() ? "" : ", ") + agent.name;
    ctx.out.line("This updates:");
    if (a) ctx.out.line("  rant-cli " RANT_CLI_VERSION " to " + cli.version + ", over " + config::to_utf8(self));
    if (ex) ctx.out.line("  the explorer " + (ex_had.empty() ? std::string("") : ex_had + " ") + "to " + ex->version + ", over " +
                         config::to_utf8(explorer::installed()));
    if (!names.empty()) ctx.out.line("  the rant skill for " + names);
    if (!ui::confirm("Update?", ctx.yes)) return 1;

    if (ex) explorer::install(ctx, *ex);
    if (!a) return 0;
    net::download(a->url, self, true);
    ctx.out.line("updated to rant-cli " + cli.version);
    /* the new binary holds the new skill */
    if (!names.empty() && process::run({ { config::to_utf8(self), "setup", "skill" }, config::fs::temp_directory_path(), {} }) != 0)
        ctx.out.warn("the rant skill was not updated, run `rant setup skill`");
    return 0;
}

/* The CLI and the explorer first, then every package of the workspace here that uses Rant,
 * each after its own question. */
static int run(app::Context& ctx) {
    if (!ctx.args.words.empty()) throw app::UsageError("update takes no folder, see `rant lib update`");
    int code = update_programs(ctx);
    const config::Workspace* ws = ctx.workspace();
    if (!ws || library::under(ws->root).empty()) return code;
    ctx.out.line();
    return library::update(ctx, ws->root, library::release("")) ? code : 1;
}

app::Command update() {
    return { "update", "", "update rant-cli and the explorer, then the Rant library of this workspace's packages",
             app::Section::Setup, {}, run };
}

}
