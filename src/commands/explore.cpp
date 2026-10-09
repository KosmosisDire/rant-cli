#include <chrono>
#include <fstream>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "config/config.hpp"
#include "net/github.hpp"
#include "process/supervisor.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;

static const char* explorer_repo = "KosmosisDire/rant-explorer";

#ifdef _WIN32
static const char* exe_name = "rant-explorer.exe";
#else
static const char* exe_name = "rant-explorer";
#endif

static fs::path installed() { return util::rant_home() / "bin" / exe_name; }
static fs::path version_file() { return util::rant_home() / "bin" / "rant-explorer.version"; }

static std::string installed_version() {
    std::ifstream in(version_file());
    std::string v;
    std::getline(in, v);
    return v;
}

/* The explorer to run: the one beside this rant first, so a pair installed together stays
 * together, then the one rant installed, then any on PATH. */
static std::optional<fs::path> locate(const app::Context& ctx) {
    fs::path beside = process::self_path().parent_path() / exe_name;
    if (fs::is_regular_file(beside)) return beside;
    if (fs::is_regular_file(installed())) return installed();
    for (const char* name : { "rant-explorer", "rant_explorer" })
        if (auto p = process::find_program(name, ctx.cwd)) return *p;
    return std::nullopt;
}

/* Downloads the release's explorer for this platform into ~/.rant/bin. */
static void install(const app::Context& ctx, const net::Release& r) {
    std::string asset = "rant-explorer-" + r.version + "-" + net::platform();
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("the explorer " + r.version + " has no build for this platform, there is no " + asset);
    ctx.out.note("downloading " + asset);
    net::download(a->url, installed(), true);
    std::ofstream(version_file()) << r.version << "\n";
    ctx.out.note("installed the explorer " + r.version + " as " + config::to_utf8(installed()));
}

static net::Release latest() { return net::release(explorer_repo); }

static int run(app::Context& ctx) {
    if (!ctx.args.words.empty()) throw app::UsageError("explore takes no words");
    std::optional<fs::path> exe = locate(ctx);
    if (!exe) {
        net::Release r = latest();
        if (!ui::confirm("The explorer is not installed. Download the explorer " + r.version + " into " +
                             config::to_utf8(installed().parent_path()) + "?",
                         ctx.yes))
            return 1;
        install(ctx, r);
        exe = installed();
    } else if (ctx.args.has("update")) {
        net::Release r = latest();
        if (installed_version() == r.version && fs::is_regular_file(installed())) ctx.out.note("the explorer " + r.version + " is the newest");
        else install(ctx, r);
        if (*exe != installed()) ctx.out.note("still starting " + config::to_utf8(*exe) + ", which comes first");
    }

    std::vector<std::string> argv = { config::to_utf8(*exe) };
    if (uint16_t d = ctx.domain()) argv.insert(argv.end(), { "--domain", std::to_string(d) });
    /* a job name of its own each time, since several explorers may run at once */
    std::string job = "Local\\rant-explorer-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    process::start_detached({ argv, ctx.cwd, {} }, util::rant_home() / "explorer.log", job);
    ctx.out.note("started " + config::to_utf8(*exe));
    return 0;
}

app::Command explore() {
    app::Command c{ "explore", "", "open the explorer, installing it first when it is missing", app::Section::Setup,
                    { { "update", 0, "", "download the newest explorer before opening it" } }, run };
    c.aliases = { "explorer" };
    return c;
}

}
