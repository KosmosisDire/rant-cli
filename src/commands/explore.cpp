#include <chrono>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "config/config.hpp"
#include "packages/packages.hpp"
#include "process/command.hpp"
#include "process/supervisor.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;

/* The explorer package's node that opens a window. */
static std::optional<config::NodeType> window_node() { return packages::installed_node("explorer", "explorer"); }

static int run(app::Context& ctx) {
    if (!ctx.args.words.empty()) throw app::UsageError("explore takes no arguments, `rant run WebExplorer` serves it as a page");
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

app::Command explore() {
    app::Command c{ "explore", "", "open the explorer, which `rant install explorer` installs", app::Section::Setup, {}, run };
    c.aliases = { "explorer" };
    return c;
}

}
