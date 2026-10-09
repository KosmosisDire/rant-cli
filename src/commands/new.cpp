#include <algorithm>
#include <optional>
#include <set>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "library/library.hpp"
#include "library/uses.hpp"
#include "scaffold/scaffold.hpp"

namespace commands {

namespace fs = std::filesystem;

static const std::vector<std::string> languages = { "cpp", "python", "csharp" };
static const std::vector<std::string> kinds = { "workspace", "package", "node", "group" };

/* The language of the package around the working directory, from its build files. */
static std::string language_here(app::Context& ctx) {
    const config::Workspace* ws = ctx.workspace();
    for (fs::path dir = ctx.cwd;; dir = dir.parent_path()) {
        if (fs::exists(dir / "CMakeLists.txt")) return "cpp";
        if (fs::exists(dir / "pyproject.toml")) return "python";
        std::error_code ec;
        for (auto& e : fs::directory_iterator(dir, ec)) {
            if (e.path().extension() == ".csproj") return "csharp";
            if (e.path().extension() == ".py") return "python";
        }
        if ((ws && dir == ws->root) || dir == dir.parent_path()) return "";
    }
}

/* The CMakeLists a C++ node made here joins: the nearest one here or above, inside the
 * workspace. */
static fs::path cmake_project(app::Context& ctx, const std::string& name) {
    const config::Workspace* ws = ctx.workspace();
    for (fs::path dir = ctx.cwd;; dir = dir.parent_path()) {
        if (fs::is_regular_file(dir / "CMakeLists.txt")) return dir / "CMakeLists.txt";
        if ((ws && dir == ws->root) || dir == dir.parent_path())
            throw app::Failure("no CMakeLists.txt here or above to add `" + name + "` to, make a package with `rant new package --lang cpp`");
    }
}

/* The preset a command line asks for. A C# node is a project of its own, so it is the
 * package preset. A workspace holds a talker, a listener and a group that runs both. */
static std::string chosen(app::Context& ctx, const std::string& kind) {
    if (kind == "group") return "group";
    std::string lang = ctx.args.get("lang").value_or(kind == "node" ? language_here(ctx) : "");
    if (lang.empty()) throw app::UsageError("say which language with --lang cpp, python or csharp");
    if (std::find(languages.begin(), languages.end(), lang) == languages.end())
        throw app::UsageError("unknown language `" + lang + "`, use cpp, python or csharp");
    if (kind == "workspace") return "workspace-" + lang;
    return (kind == "node" && lang != "csharp" ? "node-" : "package-") + lang;
}

/* Like dotnet new: a workspace or a package goes in the folder named, here by default, and
 * takes that folder's name. A node or a group is a file here, named as asked or after this
 * folder. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty() || std::find(kinds.begin(), kinds.end(), w[0]) == kinds.end())
        throw app::UsageError("say what to make: `rant new workspace`, `rant new package`, `rant new node` or `rant new group`");
    const std::string& kind = w[0];
    if (w.size() > 2) throw app::UsageError("new " + kind + " takes one folder or name");
    std::string preset = chosen(ctx, kind);
    bool folder = kind == "workspace" || kind == "package" || preset == "package-csharp";
    fs::path dest = ctx.cwd;
    if (folder && w.size() == 2) dest = (ctx.cwd / config::from_utf8(w[1])).lexically_normal();
    if (!dest.has_filename()) dest = dest.parent_path();
    std::string name = !folder && w.size() == 2 ? w[1] : config::to_utf8(dest.filename());
    scaffold::Files files = scaffold::plan(preset, dest, name);

    /* a C++ node joins the CMake project around it, which refuses a second target of its name */
    std::optional<fs::path> project;
    if (preset == "node-cpp") {
        project = cmake_project(ctx, name);
        fs::path source = fs::relative(files.front().first, project->parent_path());
        library::add_node(*project, name, source.generic_u8string());
    }
    scaffold::write(files);
    for (auto& [f, _] : files) ctx.out.line("created " + ctx.shown(f));
    if (project) ctx.out.line("added " + name + " to " + ctx.shown(*project));

    /* every folder given a build file, or a CMake project that lacks it, takes the newest
     * Rant the same way `rant lib install` adds it, since the presets name no version */
    std::set<fs::path> needs;
    for (auto& [f, _] : files)
        if (f.filename() == "CMakeLists.txt" || f.filename() == "pyproject.toml" || f.extension() == ".csproj")
            needs.insert(f.parent_path());
    if (project) {
        auto uses = library::in_folder(project->parent_path());
        auto cmake = [](const library::Use& u) { return u.kind == library::Kind::CMake; };
        if (!std::any_of(uses.begin(), uses.end(), cmake)) needs.insert(project->parent_path());
    }
    if (needs.empty()) return 0;
    bool ok = true;
    try {
        net::Release r = library::release("");
        for (auto& dir : needs) ok &= library::install(ctx, dir, false, r);
    } catch (const app::Failure& e) {
        ctx.out.warn(std::string("Rant was not added: ") + e.what());
        ok = false;
    }
    if (!ok) ctx.out.note("add it later with `rant lib install " + ctx.shown(dest) + "`");
    return ok ? 0 : 1;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { kinds };
    return {};
}

app::Command new_() {
    app::Command c{ "new", "workspace|package [folder] | node|group [name]",
                    "make a workspace or package here or in a folder, or a node or group here",
                    app::Section::Workspace,
                    { { "lang", 0, "L", "cpp, python or csharp, for a node the package's own by default" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
