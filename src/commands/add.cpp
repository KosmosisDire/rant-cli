#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "library/library.hpp"
#include "packages/packages.hpp"

namespace commands {

namespace fs = std::filesystem;

/* Adds a library to the folder named, here by default, or moves it there to another release.
 * Rant is the library when none is named. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() > 2) throw app::UsageError("add takes a library and one folder at most, such as `rant add rant cam`");
    fs::path dir = ctx.cwd();
    if (w.size() == 2) {
        dir = ctx.cwd() / config::from_utf8(w[1]);
        if (!fs::is_directory(dir)) throw app::Failure("no folder `" + w[1] + "`");
        dir = fs::weakly_canonical(dir);
    }
    packages::Sources sources;
    library::Library lib = library::find(ctx, sources, w.empty() ? library::default_library : w[0]);
    std::optional<std::string> version = ctx.args.get("version");
    return library::add(ctx, dir, lib, library::release(lib, version.value_or("")), version.has_value()) ? 0 : 1;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    std::vector<std::string> names;
    for (auto& s : packages::cached())
        if (s.library) names.push_back(s.name);
    return { names };
}

app::Command add() {
    app::Command c{ "add", "[library [folder]]",
                    "add a library to the package here or in folder, Rant when none is named, or move it to another release",
                    app::Section::Setup,
                    { { "version", 0, "V", "this release of the library, the latest by default" } }, run };
    c.complete = complete_words;
    return c;
}

}
