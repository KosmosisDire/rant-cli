#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "config/lib.hpp"
#include "library/library.hpp"
#include "ui/table.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static fs::path folder(const app::Context& ctx, const std::string& word) {
    fs::path dir = ctx.cwd / config::from_utf8(word);
    if (!fs::is_directory(dir)) throw app::Failure("no folder `" + word + "`");
    return fs::weakly_canonical(dir);
}

static int show(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() > 1) throw app::UsageError("lib takes at most one folder");
    bool one = w.size() == 1;
    fs::path dir = one ? folder(ctx, w[0]) : ctx.cwd;
    std::vector<config::LibUse> uses = config::lib_uses(dir, !one);
    if (ctx.json) {
        json out = json::array();
        for (auto& u : uses)
            out.push_back({ { "folder", config::to_utf8(u.dir) },
                            { "kind", config::lib_kind_name(u.kind) },
                            { "how", u.how },
                            { "version", u.version ? json(*u.version) : json(nullptr) },
                            { "file", config::to_utf8(u.file) } });
        ctx.out.line(out.dump(2));
        return 0;
    }
    if (uses.empty()) {
        ctx.out.note(one ? "`" + w[0] + "` does not use Rant yet, add it with `rant lib install " + w[0] + "`"
                         : "nothing under here uses Rant yet, add it to a folder with `rant lib install <folder>`");
        return 0;
    }
    ui::Table t;
    for (auto& u : uses) {
        std::string where = u.how + " " + ctx.shown(u.file);
        if (u.how == "venv" && !u.venv_exists) where = "no venv yet";
        t.row({ ctx.shown(u.dir), config::lib_kind_name(u.kind), u.version ? *u.version : ctx.out.paint(ui::Style::Dim, "none"),
                ctx.out.paint(ui::Style::Dim, where) });
    }
    for (auto& l : t.lines("")) ctx.out.line(l);
    return 0;
}

static int install(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() > 2) throw app::UsageError("lib install takes at most one folder");
    bool one = w.size() == 2;
    fs::path dir = one ? folder(ctx, w[1]) : ctx.cwd;
    config::Release r = library::release(ctx.args.get("version").value_or(""));
    return library::install(ctx, dir, !one, r) ? 0 : 1;
}

static int run(app::Context& ctx) {
    if (!ctx.args.words.empty() && ctx.args.words[0] == "install") return install(ctx);
    if (ctx.args.has("version")) throw app::UsageError("--version goes with `rant lib install`");
    return show(ctx);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { { "install" } };
    return {};
}

app::Command lib() {
    app::Command c{ "lib", "[folder] | install [folder]", "show or install the Rant library a package uses",
                    app::Section::Setup,
                    { { "version", 0, "V", "with install: this Rant version, the latest by default" } }, run };
    c.complete = complete_words;
    return c;
}

}
