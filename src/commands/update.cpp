#include <cstdlib>
#include <string>
#include <vector>

#include "agents/skill.hpp"
#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "config/config.hpp"
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

int update_self(app::Context& ctx) {
    net::Release r = net::release(cli_repo);
    if (numbers(r.version) <= numbers(RANT_CLI_VERSION)) {
        ctx.out.line("rant-cli " RANT_CLI_VERSION " is up to date, the newest release is " + r.version);
        return 0;
    }
    std::string asset = "rant-" + r.version + "-" + net::platform();
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("rant-cli " + r.version + " has no download for this platform, its release lacks " + asset);
    config::fs::path self = process::self_path();
    ctx.out.line("This updates rant-cli " RANT_CLI_VERSION " to " + r.version + ":");
    ctx.out.line("  download " + a->url);
    ctx.out.line("    over " + config::to_utf8(self));
    /* the skill moves along only where someone installed it */
    std::string names;
    for (auto& agent : agents::detected())
        if (agents::installed(agent.skills)) names += (names.empty() ? "" : ", ") + agent.name;
    if (!names.empty()) ctx.out.line("  update the rant skill for " + names);
    if (!ui::confirm("Update?", ctx.yes)) return 1;
    net::download(a->url, self, true);
    ctx.out.line("updated to rant-cli " + r.version);
    /* the new binary holds the new skill */
    if (!names.empty() && process::run({ { config::to_utf8(self), "setup", "skill" }, config::fs::temp_directory_path(), {} }) != 0)
        ctx.out.warn("the rant skill was not updated, run `rant setup skill`");
    return 0;
}

}
