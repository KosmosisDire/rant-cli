#include "library/library.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>
#include <optional>
#include <utility>
#include <vector>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace library {

namespace fs = std::filesystem;

/* A source as a library, refused when it names a package or a folder. */
static Library of(const packages::Source& s) {
    if (!s.library) throw app::Failure(s.name + " is a package, install it with `rant install " + s.name + "`");
    if (s.repo.empty()) throw app::Failure(s.name + " is a folder in sources.rant, and a library comes from a repo's releases");
    return { s.name, s.repo, *s.library };
}

Library find(const app::Context& ctx, packages::Sources& sources, const std::string& word) {
    std::optional<std::string> repo = config::is_package_name(word) ? std::nullopt : config::repo_of(word);
    if (!config::is_package_name(word) && !repo)
        throw app::UsageError("`" + word + "` is no library name, which is lowercase letters, digits and _, and no GitHub repo");
    if (repo) return of(packages::from_repo(*repo));
    std::optional<packages::Source> found = sources.find(word);
    if (!found) {
        for (auto& p : sources.problems()) ctx.out.warn(p);
        throw app::Failure("no library `" + word + "` in sources.rant or the package databases, `rant ls libraries --available` lists them");
    }
    return of(*found);
}

std::vector<Library> all(packages::Sources& sources) {
    std::vector<Library> out;
    for (auto& o : sources.offers())
        if (o.source.library && !o.source.repo.empty()) out.push_back(of(o.source));
    return out;
}

std::vector<Library> known() {
    std::vector<Library> out;
    for (auto& s : packages::cached(true))
        if (s.library && !s.repo.empty()) out.push_back(of(s));
    return out;
}

net::Release release(const Library& lib, const std::string& version) { return net::release(lib.repo, version); }

/* The wheel of a release for the platform this CLI runs on, else one for any platform. */
static const net::Asset* wheel(const net::Release& r, const Library& lib) {
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
    std::string prefix = dist_name(lib) + "-";
    const net::Asset* any = nullptr;
    for (auto& a : r.assets) {
        std::string name = a.name;
        std::transform(name.begin(), name.end(), name.begin(), [](unsigned char c) { return (char)std::tolower(c); });
        if (name.rfind(prefix, 0) != 0 || name.size() < 4 || name.compare(name.size() - 4, 4, ".whl") != 0) continue;
        if (name.find(platform) != std::string::npos) return &a;
        if (name.size() > 8 && name.compare(name.size() - 8, 8, "-any.whl") == 0) any = &a;
    }
    return any;
}

/* Makes the venv when it is missing, then pip installs the release's wheel into it. */
static void install_python(const app::Context& ctx, const Use& u, const Library& lib, const net::Release& r) {
    const net::Asset* w = wheel(r, lib);
    if (!w) throw app::Failure(lib.name + " " + r.version + " has no Python wheel for this platform");
    if (!u.venv_exists) {
        ctx.out.note("creating the venv " + ctx.shown(u.file));
        bool was_there = fs::exists(u.file);
        std::vector<std::string> argv = u.python;
        argv.insert(argv.end(), { "-m", "venv", config::to_utf8(u.file) });
        if (process::run({ argv, u.dir, {} }) != 0) {
            std::error_code ec;
            if (!was_there) fs::remove_all(u.file, ec);    /* half a venv would pass for one next time */
#ifdef __linux__
            throw app::Failure("could not create the venv " + ctx.shown(u.file) + ", on Debian and Ubuntu install python3-venv first");
#else
            throw app::Failure("could not create the venv " + ctx.shown(u.file));
#endif
        }
    }
    std::vector<std::string> pip = { config::to_utf8(u.venv_python), "-m", "pip", "install", "--upgrade",
                                     "--disable-pip-version-check", "--quiet", w->url };
    if (process::run({ pip, u.dir, {} }) != 0) throw app::Failure("pip could not install " + lib.name + " " + r.version);
}

/* The user's NuGet config, where dotnet keeps its package sources. */
static fs::path nuget_config() {
#ifdef _WIN32
    return util::env_path("APPDATA") / "NuGet" / "NuGet.Config";
#else
    return util::home_dir() / ".nuget" / "NuGet" / "NuGet.Config";
#endif
}

fs::path nuget_feed() { return (util::rant_home() / "nuget").make_preferred(); }

bool nuget_feed_added() {
    std::string text;
    {    /* closed before dotnet writes it, which Windows refuses while it is open */
        std::ifstream in(nuget_config(), std::ios::binary);
        text.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    fs::path feed = nuget_feed();
    return text.find(feed.u8string()) != std::string::npos || text.find(config::to_utf8(feed)) != std::string::npos;
}

/* A library's C# package comes with its release, not from nuget.org, so it goes to a feed
 * under ~/.rant that dotnet is told about once. */
static void install_nupkg(const app::Context& ctx, const Library& lib, const net::Release& r) {
    std::string name = lib.names.csharp + "." + r.version + ".nupkg";
    const net::Asset* a = r.asset(name);
    if (!a) throw app::Failure(lib.name + " " + r.version + " has no " + name);
    fs::path feed = nuget_feed();
    if (!fs::exists(feed / name)) net::download(a->url, feed / name);
    if (nuget_feed_added()) return;
    if (!process::find_program("dotnet", ctx.cwd())) throw app::Failure("dotnet not found, install the .NET SDK");
    ctx.out.note("adding the NuGet feed " + config::to_utf8(feed));
    if (process::run({ { "dotnet", "nuget", "add", "source", feed.u8string(), "--name", "rant" }, ctx.cwd(), {} }) != 0)
        throw app::Failure("dotnet could not add the feed " + config::to_utf8(feed));
}

/* A use the release would change. An unpinned pyproject dependency takes what its venv has,
 * and an unpinned Cargo one what its source has. Only a version asked for moves one back. */
static bool changes(const Use& u, const net::Release& r, bool asked) {
    if (!u.wanted || u.version == r.version || ((u.how == "pyproject" || u.how == "Cargo") && !u.version)) return false;
    return asked || !u.version || !net::newer(*u.version, r.version);
}

/* Brings one use to the release. Returns what it did, empty when there was nothing to do. */
static std::string bring(const app::Context& ctx, const Use& u, const Library& lib, const net::Release& r, bool asked) {
    if (!changes(u, r, asked)) {
        if (u.how == "PackageReference") install_nupkg(ctx, lib, r);    /* the feed may still lack it */
        return "";
    }
    std::string from = (u.version ? *u.version : "none") + " -> " + r.version;
    if (u.how == "venv") {
        install_python(ctx, u, lib, r);
        return from + " in " + ctx.shown(u.file);
    }
    set_version(u.file, lib, r.version);
    if (u.kind == Kind::CSharp) install_nupkg(ctx, lib, r);
    return from + " in " + ctx.shown(u.file);
}

/* The build files in a folder that could take the library but do not name it yet. */
static std::vector<std::pair<Kind, fs::path>> missing(const fs::path& dir, const Library& lib, const std::vector<Use>& uses) {
    auto used = [&](const fs::path& f) {
        for (auto& u : uses)
            if (u.file == f) return true;
        return false;
    };
    std::vector<std::pair<Kind, fs::path>> out;
    fs::path cmake = dir / "CMakeLists.txt";
    if (supports(lib, Kind::CMake) && fs::is_regular_file(cmake) && !used(cmake)) out.push_back({ Kind::CMake, cmake });
    std::error_code ec;
    if (supports(lib, Kind::CSharp))
        for (auto& e : fs::directory_iterator(dir, ec))
            if (e.path().extension() == ".csproj" && !used(e.path())) out.push_back({ Kind::CSharp, e.path() });
    fs::path cargo = dir / "Cargo.toml";
    if (supports(lib, Kind::Rust) && fs::is_regular_file(cargo) && !used(cargo)) out.push_back({ Kind::Rust, cargo });
    return out;
}

/* The build files a library goes in, as a sentence names them: "CMakeLists.txt or Cargo.toml". */
static std::string takes(const Library& lib) {
    std::vector<std::string> files;
    if (supports(lib, Kind::CMake)) files.push_back("CMakeLists.txt");
    if (supports(lib, Kind::CSharp)) files.push_back("C# project");
    if (supports(lib, Kind::Rust)) files.push_back("Cargo.toml");
    if (supports(lib, Kind::Python)) files.push_back("Python file");
    std::string s;
    for (size_t i = 0; i < files.size(); i++) s += (i == 0 ? "" : i + 1 == files.size() ? " or " : ", ") + files[i];
    return s;
}

static void report(const app::Context& ctx, const fs::path& d, Kind k, const std::string& what) {
    ctx.out.line("  " + ctx.shown(d) + " " + kind_name(k) + ": " + what);
}

static void fetch_notes(const app::Context& ctx, bool cmake, bool cargo) {
    if (cmake) ctx.out.note("CMake fetches it on the next configure, which `rant build` runs");
    if (cargo) ctx.out.note("Cargo fetches it on the next build, which `rant build` runs");
}

bool add(const app::Context& ctx, const fs::path& dir, const Library& lib, const net::Release& r, bool asked) {
    std::vector<Use> uses = in_folder(dir, { lib }, true);
    auto adds = missing(dir, lib, uses);
    if (uses.empty() && adds.empty()) throw app::Failure("no " + takes(lib) + " in " + ctx.shown(dir) + " to add " + lib.name + " to");
    std::vector<const Use*> moves;
    for (auto& u : uses)
        if (changes(u, r, asked)) moves.push_back(&u);
    if (adds.empty() && moves.empty()) {
        std::optional<std::string> newer;
        for (auto& u : uses)
            if (u.version && *u.version != r.version) newer = u.version;
        if (newer) ctx.out.note(ctx.shown(dir) + " uses " + lib.name + " " + *newer + ", newer than the release " + r.version + ", so it stays");
        else ctx.out.note(ctx.shown(dir) + " uses " + lib.name + " " + r.version + " already");
        packages::give_skill(ctx, lib.name, lib.repo, r, true);
        return true;
    }
    ctx.out.line(ctx.out.paint(ui::Style::Bold, lib.name + " " + r.version));
    bool ok = true, cmake = false, cargo = false;
    for (auto* u : moves) {
        try {
            report(ctx, u->dir, u->kind, bring(ctx, *u, lib, r, asked));
            cmake |= u->kind == Kind::CMake;
            cargo |= u->kind == Kind::Rust;
        } catch (const app::Failure& e) {
            ctx.out.warn(ctx.shown(u->dir) + " " + kind_name(u->kind) + ": " + e.what());
            ok = false;
        }
    }
    for (auto& [kind, file] : adds) {
        try {
            std::string hint = library::add(kind, file, lib, r.version, r.tag);
            if (kind == Kind::CSharp) install_nupkg(ctx, lib, r);
            report(ctx, dir, kind, "added to " + ctx.shown(file));
            if (!hint.empty()) ctx.out.note("    link it to your target: " + hint);
            cmake |= kind == Kind::CMake;
            cargo |= kind == Kind::Rust;
        } catch (const app::Failure& e) {
            ctx.out.warn(ctx.shown(dir) + " " + kind_name(kind) + ": " + e.what());
            ok = false;
        }
    }
    fetch_notes(ctx, cmake, cargo);
    packages::give_skill(ctx, lib.name, lib.repo, r, true);
    return ok;
}

bool update(const app::Context& ctx, const std::vector<Use>& uses, const std::vector<Library>& libs) {
    std::vector<const Library*> used;
    std::vector<std::string> repos;
    for (auto& lib : libs)
        if (std::any_of(uses.begin(), uses.end(), [&](const Use& u) { return u.library == lib.name; })) {
            used.push_back(&lib);
            repos.push_back(lib.repo);
        }
    std::vector<std::optional<net::Release>> latest = net::latest(repos);

    /* every package under a folder is a lot to change unasked, so the list comes first */
    bool ok = true;
    size_t n = 0;
    for (size_t k = 0; k < used.size(); k++) {
        if (!latest[k]) {
            ctx.out.warn(used[k]->name + " has no release in " + used[k]->repo);
            ok = false;
            continue;
        }
        const net::Release& r = *latest[k];
        ctx.out.line(ctx.out.paint(ui::Style::Bold, used[k]->name + " " + r.version));
        size_t before = n;
        for (auto& u : uses)
            if (u.library == used[k]->name && changes(u, r, false)) {
                report(ctx, u.dir, u.kind, (u.version ? *u.version : "none") + " -> " + r.version + " in " + ctx.shown(u.file));
                n++;
            }
        if (n == before) ctx.out.line("  " + ctx.out.paint(ui::Style::Dim, "everything here is at " + r.version + " or newer"));
    }
    if (n > 0 && !ui::confirm("Update " + std::to_string(n) + (n == 1 ? " package?" : " packages?"), ctx.yes)) {
        ctx.out.note("nothing was changed");
        return false;
    }

    bool cmake = false, cargo = false;
    size_t done = 0;
    for (size_t k = 0; k < used.size(); k++) {
        if (!latest[k]) continue;
        for (auto& u : uses) {
            if (u.library != used[k]->name) continue;
            try {
                if (bring(ctx, u, *used[k], *latest[k], false).empty()) continue;
                done++;
                cmake |= u.kind == Kind::CMake;
                cargo |= u.kind == Kind::Rust;
            } catch (const app::Failure& e) {
                ctx.out.warn(ctx.shown(u.dir) + " " + kind_name(u.kind) + ": " + e.what());
                ok = false;
            }
        }
    }
    if (done) ctx.out.line("updated " + std::to_string(done) + (done == 1 ? " package" : " packages"));
    fetch_notes(ctx, cmake, cargo);
    for (size_t k = 0; k < used.size(); k++) {
        if (!latest[k]) continue;
        try {
            packages::give_skill(ctx, used[k]->name, used[k]->repo, *latest[k], false);
        } catch (const app::Failure& e) {
            ctx.out.warn(e.what());
            ok = false;
        }
    }
    return ok;
}

}
