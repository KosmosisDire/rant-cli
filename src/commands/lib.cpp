#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "config/lib.hpp"
#include "process/command.hpp"
#include "ui/table.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::ordered_json;

static std::string shown(const app::Context& ctx, const fs::path& p) {
    std::error_code ec;
    fs::path rel = fs::relative(p, ctx.cwd, ec);
    if (ec || rel.empty()) return config::to_utf8(p);
    return rel == "." ? "." : config::to_utf8(rel);
}

static fs::path folder(const app::Context& ctx, const std::string& word) {
    fs::path dir = ctx.cwd / config::from_utf8(word);
    if (!fs::is_directory(dir)) throw app::Failure("no folder `" + word + "`");
    return fs::weakly_canonical(dir);
}

/* The wheel of a release for the platform this CLI runs on. */
static const config::Asset* wheel(const config::Release& r) {
#if defined(_WIN32) && (defined(_M_ARM64) || defined(__aarch64__))
    const char* platform = "win_arm64";
#elif defined(_WIN32)
    const char* platform = "win_amd64";
#elif defined(__APPLE__)
    const char* platform = "macosx";
#elif defined(__aarch64__)
    const char* platform = "manylinux_2_28_aarch64";
#else
    const char* platform = "manylinux_2_28_x86_64";
#endif
    for (auto& a : r.assets)
        if (a.name.size() > 4 && a.name.compare(a.name.size() - 4, 4, ".whl") == 0 && a.name.find(platform) != std::string::npos)
            return &a;
    return nullptr;
}

static void require(const config::Outcome& o) {
    if (!o.error.empty()) throw app::Failure(o.error);
}

/* Makes the venv when it is missing, then pip installs the release's wheel into it. */
static void install_python(const app::Context& ctx, const config::LibUse& u, const config::Release& r) {
    const config::Asset* w = wheel(r);
    if (!w) throw app::Failure("Rant " + r.version + " has no Python wheel for this platform");
    if (!u.venv_exists) {
        ctx.out.note("creating the venv " + shown(ctx, u.file));
        bool was_there = fs::exists(u.file);
        std::vector<std::string> argv = u.python;
        argv.insert(argv.end(), { "-m", "venv", config::to_utf8(u.file) });
        if (process::run({ argv, u.dir, {} }) != 0) {
            std::error_code ec;
            if (!was_there) fs::remove_all(u.file, ec);    /* half a venv would pass for one next time */
#ifdef __linux__
            throw app::Failure("could not create the venv " + shown(ctx, u.file) + ", on Debian and Ubuntu install python3-venv first");
#else
            throw app::Failure("could not create the venv " + shown(ctx, u.file));
#endif
        }
    }
    std::vector<std::string> pip = { config::to_utf8(u.venv_python), "-m", "pip", "install", "--upgrade",
                                     "--disable-pip-version-check", "--quiet", w->url };
    if (process::run({ pip, u.dir, {} }) != 0) throw app::Failure("pip could not install Rant " + r.version);
}

/* The user's NuGet config, where dotnet keeps its package sources. */
static fs::path nuget_config() {
#ifdef _WIN32
    return util::env_path("APPDATA") / "NuGet" / "NuGet.Config";
#else
    return util::home_dir() / ".nuget" / "NuGet" / "NuGet.Config";
#endif
}

/* The Rant package is not on nuget.org yet, so it goes to a feed under ~/.rant that dotnet
 * is told about once. */
static void install_nupkg(const app::Context& ctx, const config::Release& r) {
    std::string name = "Rant." + r.version + ".nupkg";
    const config::Asset* a = r.asset(name);
    if (!a) throw app::Failure("Rant " + r.version + " has no " + name);
    fs::path feed = (util::rant_home() / "nuget").make_preferred();
    if (!fs::exists(feed / name)) require(config::download(a->url, feed / name));

    std::string text;
    {    /* closed before dotnet writes it, which Windows refuses while it is open */
        std::ifstream in(nuget_config(), std::ios::binary);
        text.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    if (text.find(feed.u8string()) != std::string::npos || text.find(config::to_utf8(feed)) != std::string::npos) return;
    if (!process::find_program("dotnet", ctx.cwd)) throw app::Failure("adding the Rant NuGet feed needs the .NET SDK");
    ctx.out.note("adding the NuGet feed " + config::to_utf8(feed));
    if (process::run({ { "dotnet", "nuget", "add", "source", feed.u8string(), "--name", "rant" }, ctx.cwd, {} }) != 0)
        throw app::Failure("dotnet could not add the feed " + config::to_utf8(feed));
}

static config::Release find_release(const app::Context& ctx) {
    std::string wanted = ctx.args.get("version").value_or("");
    config::Release r = config::release(config::rant_repo, wanted);
    if (!r.error.empty()) throw app::Failure(r.error);
    return r;
}

static std::string version_text(const std::optional<std::string>& v) { return v ? *v : "none"; }

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
        std::string where = u.how + " " + shown(ctx, u.file);
        if (u.how == "venv" && !u.venv_exists) where = "no venv yet";
        t.row({ shown(ctx, u.dir), config::lib_kind_name(u.kind), u.version ? *u.version : ctx.out.paint(ui::Style::Dim, "none"),
                ctx.out.paint(ui::Style::Dim, where) });
    }
    for (auto& l : t.lines("")) ctx.out.line(l);
    return 0;
}

/* Brings one use to the release. Returns what it did, empty when there was nothing to do. */
static std::string update(const app::Context& ctx, const config::LibUse& u, const config::Release& r) {
    if (u.version == r.version) {
        if (u.how == "PackageReference") install_nupkg(ctx, r);    /* the feed may still lack it */
        return "";
    }
    std::string from = version_text(u.version) + " -> " + r.version;
    if (u.how == "venv") {
        install_python(ctx, u, r);
        return from + " in " + shown(ctx, u.file);
    }
    if (u.how == "pyproject" && !u.version) return "";    /* an unpinned dependency takes what the venv has */
    require(config::lib_set(u.file, r.version));
    if (u.kind == config::LibKind::CSharp) install_nupkg(ctx, r);
    return from + " in " + shown(ctx, u.file);
}

/* The build files in a folder that could take Rant but do not name it yet. */
static std::vector<std::pair<config::LibKind, fs::path>> missing(const fs::path& dir, const std::vector<config::LibUse>& uses) {
    auto used = [&](const fs::path& f) {
        for (auto& u : uses)
            if (u.file == f) return true;
        return false;
    };
    std::vector<std::pair<config::LibKind, fs::path>> out;
    fs::path cmake = dir / "CMakeLists.txt";
    if (fs::is_regular_file(cmake) && !used(cmake)) out.push_back({ config::LibKind::CMake, cmake });
    std::error_code ec;
    for (auto& e : fs::directory_iterator(dir, ec))
        if (e.path().extension() == ".csproj" && !used(e.path())) out.push_back({ config::LibKind::CSharp, e.path() });
    return out;
}

static int install(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() > 2) throw app::UsageError("lib install takes at most one folder");
    bool one = w.size() == 2;
    fs::path dir = one ? folder(ctx, w[1]) : ctx.cwd;
    std::vector<config::LibUse> uses = config::lib_uses(dir, !one);
    auto adds = one ? missing(dir, uses) : decltype(missing(dir, uses)){};
    if (uses.empty() && adds.empty())
        throw app::Failure(one ? "no CMakeLists.txt, C# project or Python file in `" + w[1] + "`"
                               : "nothing under here uses Rant yet, add it to a folder with `rant lib install <folder>`");

    config::Release r = find_release(ctx);
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "Rant " + r.version));
    bool failed = false, cmake = false;
    auto report = [&](const fs::path& d, config::LibKind k, const std::string& what) {
        ctx.out.line("  " + shown(ctx, d) + " " + config::lib_kind_name(k) + ": " + what);
    };
    for (auto& u : uses) {
        try {
            std::string did = update(ctx, u, r);
            if (did.empty()) {
                if (u.version) report(u.dir, u.kind, ctx.out.paint(ui::Style::Dim, "already " + *u.version));
                continue;
            }
            report(u.dir, u.kind, did);
            cmake |= u.kind == config::LibKind::CMake;
        } catch (const app::Failure& e) {
            ctx.out.warn(shown(ctx, u.dir) + " " + config::lib_kind_name(u.kind) + ": " + e.what());
            failed = true;
        }
    }
    for (auto& [kind, file] : adds) {
        try {
            config::Outcome o = config::lib_add(kind, file, r.version);
            require(o);
            if (kind == config::LibKind::CSharp) install_nupkg(ctx, r);
            report(dir, kind, "added to " + shown(ctx, file));
            if (!o.note.empty()) ctx.out.note("    link it to your target: " + o.note);
            cmake |= kind == config::LibKind::CMake;
        } catch (const app::Failure& e) {
            ctx.out.warn(shown(ctx, dir) + " " + config::lib_kind_name(kind) + ": " + e.what());
            failed = true;
        }
    }
    if (cmake) ctx.out.note("CMake fetches it on the next configure, which `rant build` runs");
    return failed ? 1 : 0;
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
