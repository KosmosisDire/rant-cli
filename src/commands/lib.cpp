#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "library/library.hpp"
#include "library/uses.hpp"
#include "ui/table.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static fs::path folder(const app::Context& ctx, const std::string& word) {
    fs::path dir = ctx.cwd() / config::from_utf8(word);
    if (!fs::is_directory(dir)) throw app::Failure("no folder `" + word + "`");
    return fs::weakly_canonical(dir);
}

static int show(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() > 1) throw app::UsageError("lib takes at most one folder");
    bool one = w.size() == 1;
    fs::path dir = one ? folder(ctx, w[0]) : ctx.cwd();
    std::vector<library::Use> uses = one ? library::in_folder(dir) : library::under(dir);
    if (ctx.json) {
        json out = json::array();
        for (auto& u : uses)
            out.push_back({ { "folder", config::to_utf8(u.dir) },
                            { "kind", library::kind_name(u.kind) },
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
        t.row({ ctx.shown(u.dir), library::kind_name(u.kind), u.version ? *u.version : ctx.out.paint(ui::Style::Dim, "none"),
                ctx.out.paint(ui::Style::Dim, where) });
    }
    for (auto& l : t.lines("")) ctx.out.line(l);
    return 0;
}

/* install adds Rant to the folder named, here by default. update moves every package under
 * the folder named, the whole workspace by default, to another release. */
static int change(app::Context& ctx) {
    auto& w = ctx.args.words;
    bool install = w[0] == "install";
    if (w.size() > 2) throw app::UsageError("lib " + w[0] + " takes at most one folder");
    fs::path dir = ctx.cwd();
    if (w.size() == 2) dir = folder(ctx, w[1]);
    else if (!install && ctx.workspace()) dir = ctx.workspace()->root;
    net::Release r = library::release(ctx.args.get("version").value_or(""));
    return (install ? library::install(ctx, dir, r) : library::update(ctx, dir, r)) ? 0 : 1;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (!w.empty() && (w[0] == "install" || w[0] == "update")) return change(ctx);
    if (ctx.args.has("version")) throw app::UsageError("--version goes with `rant lib install` or `rant lib update`");
    return show(ctx);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { { "install", "update" } };
    return {};
}

app::Command lib() {
    app::Command c{ "lib", "[folder] | install [folder] | update [folder]",
                    "show the Rant library packages use, add it where it is missing, or update it",
                    app::Section::Setup,
                    { { "version", 0, "V", "with install or update: this Rant version, the latest by default" } }, run };
    c.complete = complete_words;
    return c;
}

}
