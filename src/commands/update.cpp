#include <cstdlib>
#include <optional>
#include <string>
#include <vector>

#include "agents/skill.hpp"
#include "app/failure.hpp"
#include "commands/bridge.hpp"
#include "commands/commands.hpp"
#include "commands/explore.hpp"
#include "commands/vscode.hpp"
#include "config/config.hpp"
#include "library/library.hpp"
#include "library/uses.hpp"
#include "net/github.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

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

/* Replaces this binary, the explorer, the bridge and the VS Code extension rant installed
 * with their newest releases, after one question, never with an older one. The skill moves
 * with the CLI, all only where someone installed them. */
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
    std::optional<net::Release> br;
    std::string br_had = service::installed_version(bridge::service());
    if (config::fs::is_regular_file(bridge::installed())) {
        try {
            net::Release latest = bridge::latest();
            if (numbers(latest.version) > numbers(br_had)) br = latest;
        } catch (const app::Failure& e) {
            ctx.out.warn(std::string("cannot check the bridge: ") + e.what());
        }
    }
    std::optional<net::Release> vs;
    std::optional<config::fs::path> editor = vscode::recorded_editor();
    std::string vs_had = editor ? vscode::installed_version(*editor) : "";
    if (!vs_had.empty()) {
        try {
            net::Release latest = vscode::latest();
            if (numbers(latest.version) > numbers(vs_had)) vs = latest;
        } catch (const app::Failure& e) {
            ctx.out.warn(std::string("cannot check the VS Code extension: ") + e.what());
        }
    }
    if (!a && !ex && !br && !vs) {
        std::vector<std::string> have = { "rant-cli " RANT_CLI_VERSION };
        if (config::fs::is_regular_file(explorer::installed())) have.push_back("the explorer " + ex_had);
        if (config::fs::is_regular_file(bridge::installed())) have.push_back("the bridge " + br_had);
        if (!vs_had.empty()) have.push_back("the VS Code extension " + vs_had);
        std::string list;
        for (size_t i = 0; i < have.size(); i++) list += (i == 0 ? "" : i + 1 == have.size() ? " and " : ", ") + have[i];
        ctx.out.line(list + (have.size() > 1 ? " are" : " is") + " up to date");
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
    if (br) ctx.out.line("  the bridge " + (br_had.empty() ? std::string("") : br_had + " ") + "to " + br->version + ", over " +
                         config::to_utf8(bridge::installed()));
    if (vs) ctx.out.line("  the VS Code extension " + vs_had + " to " + vs->version + ", through " + config::to_utf8(*editor));
    if (!names.empty()) ctx.out.line("  the rant skill for " + names);
    if (!ui::confirm("Update?", ctx.yes)) return 1;

    if (ex) explorer::install(ctx, *ex);
    if (vs) vscode::install(ctx, *editor, *vs);
    if (br) {
        bool was_running = service::running(bridge::service()).has_value();
        bridge::install(ctx, *br);
        if (was_running) {
            try {
                service::restart(bridge::service());
                ctx.out.note("restarted the bridge");
            } catch (const app::Failure& e) {
                ctx.out.warn(std::string("the bridge was not restarted: ") + e.what());
            }
        }
    }
    if (!a) return 0;
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
    return 0;
}

/* The CLI, the explorer, the bridge and the VS Code extension first, then every package of
 * the workspace here that uses Rant, each after its own question. */
static int run(app::Context& ctx) {
    if (!ctx.args.words.empty()) throw app::UsageError("update takes no folder, see `rant lib update`");
    int code = update_programs(ctx);
    const config::Workspace* ws = ctx.workspace();
    if (!ws || library::under(ws->root).empty()) return code;
    ctx.out.line();
    return library::update(ctx, ws->root, library::release("")) ? code : 1;
}

app::Command update() {
    return { "update", "", "update rant-cli, the explorer, the bridge and the VS Code extension, then the Rant library of this workspace's packages",
             app::Section::Setup, {}, run };
}

}
