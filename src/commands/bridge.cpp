#include <fstream>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/bridge.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static const char* bridge_repo = "KosmosisDire/rant-bridge";

#ifdef _WIN32
static const char* exe_name = "rant-bridge.exe";
#else
static const char* exe_name = "rant-bridge";
#endif

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

/* A path as a program or a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

namespace bridge {

fs::path installed() { return util::rant_home() / "bin" / exe_name; }
fs::path config_path() { return util::rant_home() / "bridge.yaml"; }

service::Service service() {
    service::Service s;
    s.name = "the bridge";
    s.command = "bridge";
    s.description = "Rant bridge";
    s.exe = installed();
    s.version_file = util::rant_home() / "bin" / "rant-bridge.version";
    s.log = util::rant_home() / "logs" / "bridge.log";
    s.args = { "--config", native(config_path()) };
    /* the bridge supervises itself there, since the Run key starts it once */
    s.windows_args = { "--supervise", "--log-file", native(s.log) };
    s.state = util::rant_home() / "bridge.state";
    s.unit = "rant-bridge";
    s.label = "dev.rantlib.bridge";
    s.run_name = "RantBridge";
    s.stop_event = "Local\\rant-bridge-stop";
    s.stays = "its binary and " + u8(config_path()) + " stay";
    return s;
}

void install(const app::Context& ctx, const net::Release& r) {
    std::string asset = "rant-bridge-" + r.version + "-" + net::platform();
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("the bridge " + r.version + " release has no " + asset);
    ctx.out.note("downloading " + asset);
    net::download(a->url, installed(), true);
    std::ofstream(service().version_file) << r.version << "\n";
    ctx.out.note("installed the bridge " + r.version + " as " + u8(installed()));
}

net::Release latest() { return net::release(bridge_repo); }

}

/* The bridge binary in place: the file given, else the one installed, else the newest release. */
static void ensure_binary(app::Context& ctx, const service::Service& s) {
    if (auto from = ctx.args.get("from")) {
        service::install_from(ctx, s, *from);
    } else if (!fs::is_regular_file(s.exe)) {
        net::Release r;
        try {
            r = bridge::latest();
        } catch (const app::Failure& e) {
            throw app::Failure(std::string(e.what()) + ". To use a bridge you built, run `rant bridge enable --from PATH`");
        }
        bridge::install(ctx, r);
    }
}

/* The settings enable passes to bridge.yaml through the bridge, which checks them. */
static const std::vector<std::string> settings = { "bind", "port", "pass" };

static int bridge_enable(app::Context& ctx, const service::Service& s) {
    ensure_binary(ctx, s);
    std::error_code ec;
    fs::create_directories(s.log.parent_path(), ec);
    std::vector<std::string> init = { native(s.exe), "--init-config", "--config", native(bridge::config_path()) };
    for (auto& o : settings)
        if (auto v = ctx.args.get(o)) init.insert(init.end(), { "--" + o, *v });
    bool changed = init.size() > 4;
    if (!process::output({ init, util::rant_home(), {} }))
        throw app::Failure(changed ? "the bridge did not take the settings, an older one takes none, `rant update` installs the newest"
                                   : "`rant-bridge --init-config` failed, run it by hand to see why");

    /* a running bridge reads its config only as it starts */
    std::optional<service::State> was = service::running(s);
    service::define(s);
    if (was && changed) service::restart(s);
    else if (!was) service::start(s);
    service::State st = service::wait_listening(s, was && changed ? was->pid : 0);
    ctx.out.note("enabled the bridge, it starts when you log in");
    service::note_linger(ctx);
    print_value(ctx, json{ { "url", st.url }, { "config", u8(bridge::config_path()) } });
    return 0;
}

static int bridge_status(app::Context& ctx, const service::Service& s) {
    std::optional<service::State> st = service::running(s);
    std::string version = service::installed_version(s);
    bool installed = fs::is_regular_file(s.exe);
    json j = { { "installed", installed },
               { "binary", u8(s.exe) },
               { "version", version.empty() ? json(nullptr) : json(version) },
               { "enabled", service::enabled(s) },
               { "running", st.has_value() },
               { "url", st ? json(st->url) : json(nullptr) },
               { "pid", st ? json(st->pid) : json(nullptr) },
               { "config", u8(bridge::config_path()) } };
    print_value(ctx, j);
    if (!installed && !ctx.json) ctx.out.note("the bridge is not installed, run `rant bridge enable`");
    return 0;
}

static int run(app::Context& ctx) {
    service::Service s = bridge::service();
    auto& w = ctx.args.words;
    std::vector<std::string> enable_options = settings;
    enable_options.push_back("from");
    service::check_usage(ctx, s, w, enable_options);
    if (w[0] == "enable") return bridge_enable(ctx, s);
    if (w[0] == "status") return bridge_status(ctx, s);
    return *service::run_shared(ctx, s, w);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { service::subcommands() };
}

app::Command bridge_() {
    app::Command c{ "bridge", "enable | disable | start | stop | restart | status | logs | run [-- args...]",
                    "run the bridge, a background server that connects web frontends to the mesh, config in ~/.rant/bridge.yaml",
                    app::Section::Setup,
                    { { "from", 0, "PATH", "with enable: install this rant-bridge instead of downloading the newest release" },
                      { "bind", 0, "IP", "with enable: listen on this address, 0.0.0.0 every interface (the default), 127.0.0.1 this computer only" },
                      { "port", 0, "N", "with enable: listen on this port, 7480 by default" },
                      { "pass", 0, "TEXT", "with enable: the passphrase clients give, \"\" for none" },
                      { "follow", 'f', "", "with logs: keep printing new lines until Ctrl-C" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
