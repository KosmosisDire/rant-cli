#include <cstdlib>
#include <fstream>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/bridge.hpp"
#include "commands/commands.hpp"
#include "commands/explore.hpp"
#include "commands/format.hpp"
#include "commands/web.hpp"
#include "config/config.hpp"
#include "packages/packages.hpp"
#include "process/command.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

/* A path as a program or a service definition reads it, with the separators of this OS. */
static std::string native(const fs::path& p) { return p.u8string(); }

namespace web {

/* Where the server listens, as `rant explore web enable` saved it: one line, the bind address and the port. */
struct Address {
    std::string bind = "0.0.0.0";
    int         port = 6732;     /* Rant's ports are 6730 to 6739 */
};

static fs::path address_file() { return util::rant_home() / "explorer-web.address"; }

static Address saved() {
    Address a;
    std::ifstream in(address_file());
    Address read;
    if (in >> read.bind >> read.port && read.port > 0 && read.port < 65536) a = read;
    return a;
}

static service::Service service_at(const Address& a) {
    service::Service s;
    s.name = "the web explorer";
    s.command = "explore web";
    s.description = "Rant web explorer";
    s.exe = explorer::program().value_or(fs::path());
    s.log = util::rant_home() / "logs" / "explorer-web.log";
    s.args = { "--web-server", "--port", std::to_string(a.port), "--bind", a.bind };
    s.windows_args = s.args;
    s.windows_args.insert(s.windows_args.end(), { "--log-file", native(s.log) });
    s.state = util::rant_home() / "explorer-web.state";
    s.unit = "rant-explorer-web";
    s.label = "dev.rantlib.explorer-web";
    s.run_name = "RantExplorerWeb";
    s.stop_event = "Local\\rant-explorer-web-stop";
    s.stays = "the explorer stays installed";
    s.installs = "rant install explorer";
    return s;
}

service::Service service() { return service_at(saved()); }

}

/* The address to serve on: what enable was given, else what it saved before, else the defaults. */
static web::Address requested(const app::Context& ctx) {
    web::Address a = web::saved();
    if (auto bind = ctx.args.get("bind")) {
        if (bind->empty() || bind->find_first_of(" \t") != std::string::npos)
            throw app::UsageError("--bind takes an address such as 0.0.0.0 or 127.0.0.1");
        a.bind = *bind;
    }
    if (auto port = ctx.args.get("port")) {
        char* end = nullptr;
        long p = std::strtol(port->c_str(), &end, 10);
        if (port->empty() || *end || p < 1 || p > 65535) throw app::UsageError("--port takes a number from 1 to 65535");
        a.port = (int)p;
    }
    return a;
}

/* --version came in the same release as --web-server, so an explorer that refuses it cannot serve. */
static void ensure_explorer(const service::Service& s) {
    service::require_installed(s);
    if (!process::output({ { native(s.exe), "--version" }, fs::temp_directory_path(), {} }))
        throw app::Failure("this explorer has no web server, `rant update explorer` installs the newest");
}

static int web_enable(app::Context& ctx) {
    web::Address a = requested(ctx);
    service::Service s = web::service_at(a);
    ensure_explorer(s);
    std::error_code ec;
    fs::create_directories(s.log.parent_path(), ec);
    std::ofstream(web::address_file()) << a.bind << " " << a.port << "\n";

    /* a running server may listen elsewhere, so it starts again on the address given */
    std::optional<service::State> was = service::running(s);
    service::define(s);
    if (was) service::restart(s);
    else service::start(s);
    service::State st = service::wait_listening(s, was ? was->pid : 0);
    ctx.out.note("enabled the web explorer, it starts when you log in");
    service::note_linger(ctx);
    if (a.bind == "0.0.0.0") ctx.out.note("other devices open it at this machine's address, port " + std::to_string(a.port));
    if (!service::running(bridge::service())) ctx.out.note("the page reaches the mesh through the bridge, run `rant bridge enable`");
    print_value(ctx, json{ { "url", st.url } });
    return 0;
}

static int web_status(app::Context& ctx, const service::Service& s) {
    std::optional<service::State> st = service::running(s);
    std::optional<packages::Record> rec = packages::record("explorer");
    std::string version = rec ? rec->version : "";
    bool installed = fs::is_regular_file(s.exe);
    web::Address a = web::saved();
    json j = { { "installed", installed },
               { "binary", u8(s.exe) },
               { "version", version.empty() ? json(nullptr) : json(version) },
               { "enabled", service::enabled(s) },
               { "running", st.has_value() },
               { "url", st ? json(st->url) : json(nullptr) },
               { "pid", st ? json(st->pid) : json(nullptr) },
               { "bind", a.bind },
               { "port", a.port } };
    print_value(ctx, j);
    if (!installed && !ctx.json) ctx.out.note("the explorer is not installed, run `rant install explorer`");
    return 0;
}

namespace web {

const std::vector<std::string>& options() {
    static const std::vector<std::string> list = { "port", "bind" };
    return list;
}

int run(app::Context& ctx, const std::vector<std::string>& words) {
    service::Service s = service();
    service::check_usage(ctx, s, words, options());
    if (words[0] == "enable") return web_enable(ctx);
    if (words[0] == "status") return web_status(ctx, s);
    return *service::run_shared(ctx, s, words);
}

}

}
