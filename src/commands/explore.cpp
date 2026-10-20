#include <chrono>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/explore.hpp"
#include "commands/web.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "packages/packages.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;

/* The explorer package's node that opens a window. */
static std::optional<config::NodeType> window_node() { return packages::installed_node("explorer", "explorer"); }

namespace explorer {

std::optional<fs::path> program() {
    std::optional<config::NodeType> n = window_node();
    if (!n || n->run.empty()) return std::nullopt;
    return process::find_program(n->run[0], n->cwd);
}

}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (!w.empty() && w[0] == "web") return web::run(ctx, { w.begin() + 1, w.end() });
    if (!w.empty()) throw app::UsageError("explore takes no arguments but `web`, see `rant explore --help`");
    for (auto& o : web::options())
        if (ctx.args.has(o)) throw app::UsageError("--" + o + " goes with `rant explore web enable`");
    if (ctx.args.has("follow")) throw app::UsageError("--follow goes with `rant explore web logs`");
    std::optional<config::NodeType> node = window_node();
    if (!node || node->run.empty()) throw app::Failure("the explorer is not installed, `rant install explorer` installs it");

    std::vector<std::string> argv = node->run;
    if (uint16_t d = ctx.domain()) argv.insert(argv.end(), { "--domain", std::to_string(d) });
    /* a job name of its own each time, since several explorers may run at once */
    std::string job = "Local\\rant-explorer-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    process::start_detached({ argv, node->cwd, {} }, util::rant_home() / "explorer.log", job);
    ctx.out.note("started " + argv[0]);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { { "web" } };
    if (r.words.size() == 1 && r.words[0] == "web") return { service::subcommands() };
    return {};
}

app::Command explore() {
    app::Command c{ "explore", "[web enable | disable | start | stop | restart | status | logs | run [-- args...]]",
                    "open the explorer, which `rant install explorer` installs. "
                    "`web` serves it as a page in the background, for any browser on the network",
                    app::Section::Setup,
                    { { "port", 0, "N", "with web enable: the port to serve on, 6732 unless set before" },
                      { "bind", 0, "IP", "with web enable: the address to serve on, 0.0.0.0 (every address) unless set before" },
                      { "follow", 'f', "", "with web logs: keep printing new lines until Ctrl-C" } },
                    run };
    c.aliases = { "explorer" };
    c.complete = complete_words;
    return c;
}

}
