#include <algorithm>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "complete/complete.hpp"
#include "library/library.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;

static const std::vector<std::string> languages = { "cpp", "python", "csharp" };

/* Where a user's own templates live, one folder each. */
static fs::path user_templates() {
    fs::path config = util::env_path("XDG_CONFIG_HOME");
    if (config.empty()) config = util::home_dir() / ".config";
    return config / "rant" / "templates";
}

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

/* A template named on the command line, fetched with git when it is a repository. The
 * clone sits in ~/.rant/cache until the next one. */
static config::TemplateOrigin named_template(app::Context& ctx, const std::string& name) {
    std::string url;
    if (name.rfind("gh:", 0) == 0) url = "https://github.com/" + name.substr(3) + ".git";
    else if (name.find("://") != std::string::npos || name.rfind("git@", 0) == 0) url = name;
    if (!url.empty()) {
        fs::path clone = util::rant_home() / "cache" / "template";
        std::error_code ec;
        fs::remove_all(clone, ec);
        if (!process::find_program("git", ctx.cwd)) throw app::Failure("a template from a repository needs git");
        if (process::run({ { "git", "clone", "--depth", "1", "--quiet", url, config::to_utf8(clone) }, ctx.cwd, {} }) != 0)
            throw app::Failure("git could not clone " + url);
        return { "", clone };
    }
    fs::path local = ctx.cwd / config::from_utf8(name);
    if (fs::is_directory(local)) return { "", local };
    fs::path own = user_templates() / config::from_utf8(name);
    if (fs::is_directory(own)) return { "", own };
    throw app::Failure("no template `" + name + "`: not a folder here, nor in " + config::to_utf8(user_templates()));
}

/* The template a command line asks for. A C# node is a project of its own, so it is the
 * package template. */
static config::TemplateOrigin chosen(app::Context& ctx, const std::string& kind) {
    if (auto t = ctx.args.get("template")) return named_template(ctx, *t);
    if (kind == "group") return { "group", {} };
    std::string lang = ctx.args.get("lang").value_or(kind == "node" ? language_here(ctx) : "");
    if (lang.empty()) throw app::UsageError("say which language with --lang cpp, python or csharp");
    if (std::find(languages.begin(), languages.end(), lang) == languages.end())
        throw app::UsageError("unknown language `" + lang + "`, use cpp, python or csharp");
    return { (kind == "node" && lang != "csharp" ? "node-" : "package-") + lang, {} };
}

static config::TemplateInfo describe(app::Context& ctx, const config::TemplateOrigin& origin) {
    config::TemplateInfo t = config::describe_template(origin);
    if (!t.diagnostics.empty()) {
        for (auto& d : t.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    return t;
}

/* Asks for each required param not given, when someone is there to answer. */
static void ask_missing(const config::TemplateInfo& t, std::vector<std::string>& given) {
    for (auto& p : t.params) {
        bool set = std::any_of(given.begin(), given.end(), [&](const std::string& g) { return g.rfind(p.name + "=", 0) == 0; });
        if (set || p.default_value) continue;
        std::string question = p.name + (p.description.empty() ? "" : " (" + p.description + ")") + "?";
        if (auto answer = ui::ask(question)) given.push_back(p.name + "=" + *answer);
    }
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty() || (w[0] != "package" && w[0] != "node" && w[0] != "group"))
        throw app::UsageError("say what to make: `rant new package <name>`, `rant new node <name>` or `rant new group <name>`");
    const std::string& kind = w[0];
    if (w.size() < 2) throw app::UsageError("new " + kind + " needs a name");
    const std::string& name = w[1];
    config::TemplateOrigin origin = chosen(ctx, kind);
    config::TemplateInfo t = describe(ctx, origin);
    if (ctx.args.has("help")) {
        ctx.out.line("Usage: rant new " + kind + " " + name + (t.params.empty() ? "" : " [key=value...]"));
        if (!t.description.empty()) ctx.out.line("\n" + t.description);
        print_params(ctx, t.params);
        return 0;
    }

    std::vector<std::string> params(w.begin() + 2, w.end());
    ask_missing(t, params);
    bool package = kind == "package" || origin.builtin == "package-csharp";
    fs::path dest = package ? ctx.cwd / config::from_utf8(name) : ctx.cwd;
    config::Made made = config::make(origin, dest, name, params);
    if (!made.diagnostics.empty()) {
        for (auto& d : made.diagnostics) ctx.out.error(d.str());
        throw app::Failure("");
    }
    for (auto& f : made.files) ctx.out.line("created " + ctx.shown(f));
    if (!made.next.empty()) ctx.out.note(made.next);
    if (!package) return 0;

    /* the template names no Rant version, the same routine as `rant lib install` adds the newest */
    try {
        return library::install(ctx, dest, false, library::release("")) ? 0 : 1;
    } catch (const app::Failure& e) {
        ctx.out.warn(std::string("Rant was not added: ") + e.what());
        ctx.out.note("add it later with `rant lib install " + ctx.shown(dest) + "`");
        return 1;
    }
}

static complete::Candidates complete_words(complete::Request& r) {
    if (r.words.empty()) return { { "package", "node", "group" } };
    return {};
}

app::Command new_() {
    app::Command c{ "new", "package|node|group <name> [key=value...]", "make a package, node or group from a template",
                    app::Section::Workspace,
                    { { "lang", 0, "L", "cpp, python or csharp, for a node the package's own by default" },
                      { "template", 0, "T", "a folder, a template in ~/.config/rant/templates or gh:owner/repo" } },
                    run };
    c.own_help = true;
    c.complete = complete_words;
    return c;
}

}
