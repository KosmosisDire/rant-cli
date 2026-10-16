#include "library/library.hpp"

#include <fstream>
#include <iterator>
#include <optional>
#include <sstream>
#include <utility>
#include <vector>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "library/uses.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace library {

namespace fs = std::filesystem;

/* The wheel of a release for the platform this CLI runs on. */
static const net::Asset* wheel(const net::Release& r) {
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

/* Makes the venv when it is missing, then pip installs the release's wheel into it. */
static void install_python(const app::Context& ctx, const Use& u, const net::Release& r) {
    const net::Asset* w = wheel(r);
    if (!w) throw app::Failure("Rant " + r.version + " has no Python wheel for this platform");
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

/* The Rant package is not on nuget.org yet, so it goes to a feed under ~/.rant that dotnet
 * is told about once. */
static void install_nupkg(const app::Context& ctx, const net::Release& r) {
    std::string name = "Rant." + r.version + ".nupkg";
    const net::Asset* a = r.asset(name);
    if (!a) throw app::Failure("Rant " + r.version + " has no " + name);
    fs::path feed = nuget_feed();
    if (!fs::exists(feed / name)) net::download(a->url, feed / name);
    if (nuget_feed_added()) return;
    if (!process::find_program("dotnet", ctx.cwd())) throw app::Failure("dotnet not found, install the .NET SDK");
    ctx.out.note("adding the NuGet feed " + config::to_utf8(feed));
    if (process::run({ { "dotnet", "nuget", "add", "source", feed.u8string(), "--name", "rant" }, ctx.cwd(), {} }) != 0)
        throw app::Failure("dotnet could not add the feed " + config::to_utf8(feed));
}

net::Release release(const std::string& version) { return net::release(rant_repo, version); }

/* The first Rant release with a crate-v tag, the packaged Rust crate a Cargo package fetches. */
static const char* first_rust_release = "0.0.24";

/* a before b, for dotted versions such as 0.0.23 */
static bool older(const std::string& a, const std::string& b) {
    std::istringstream x(a), y(b);
    for (std::string p, q;;) {
        bool more_a = (bool)std::getline(x, p, '.'), more_b = (bool)std::getline(y, q, '.');
        if (!more_a || !more_b) return !more_a && more_b;
        long m = std::strtol(p.c_str(), nullptr, 10), n = std::strtol(q.c_str(), nullptr, 10);
        if (m != n) return m < n;
    }
}

static void require_rust(const net::Release& r) {
    if (older(r.version, first_rust_release))
        throw app::Failure("Rant " + r.version + " has no Rust crate, Rust needs Rant " + first_rust_release + " or later");
}

/* A use the release would change. An unpinned pyproject dependency takes what its venv has,
 * and an unpinned Cargo one what its source has. */
static bool changes(const Use& u, const net::Release& r) {
    return u.wanted && u.version != r.version && !((u.how == "pyproject" || u.how == "Cargo") && !u.version);
}

/* Brings one use to the release. Returns what it did, empty when there was nothing to do. */
static std::string bring(const app::Context& ctx, const Use& u, const net::Release& r) {
    if (!changes(u, r)) {
        if (u.how == "PackageReference") install_nupkg(ctx, r);    /* the feed may still lack it */
        return "";
    }
    std::string from = (u.version ? *u.version : "none") + " -> " + r.version;
    if (u.kind == Kind::Rust) require_rust(r);
    if (u.how == "venv") {
        install_python(ctx, u, r);
        return from + " in " + ctx.shown(u.file);
    }
    set_version(u.file, r.version);
    if (u.kind == Kind::CSharp) install_nupkg(ctx, r);
    return from + " in " + ctx.shown(u.file);
}

/* The build files in a folder that could take Rant but do not name it yet. */
static std::vector<std::pair<Kind, fs::path>> missing(const fs::path& dir, const std::vector<Use>& uses) {
    auto used = [&](const fs::path& f) {
        for (auto& u : uses)
            if (u.file == f) return true;
        return false;
    };
    std::vector<std::pair<Kind, fs::path>> out;
    fs::path cmake = dir / "CMakeLists.txt";
    if (fs::is_regular_file(cmake) && !used(cmake)) out.push_back({ Kind::CMake, cmake });
    std::error_code ec;
    for (auto& e : fs::directory_iterator(dir, ec))
        if (e.path().extension() == ".csproj" && !used(e.path())) out.push_back({ Kind::CSharp, e.path() });
    fs::path cargo = dir / "Cargo.toml";
    if (fs::is_regular_file(cargo) && !used(cargo)) out.push_back({ Kind::Rust, cargo });
    return out;
}

static void report(const app::Context& ctx, const fs::path& d, Kind k, const std::string& what) {
    ctx.out.line("  " + ctx.shown(d) + " " + kind_name(k) + ": " + what);
}

bool install(const app::Context& ctx, const fs::path& dir, const net::Release& r) {
    std::vector<Use> uses = in_folder(dir);
    auto adds = missing(dir, uses);
    if (uses.empty() && adds.empty()) throw app::Failure("no CMakeLists.txt, C# project, Cargo.toml or Python file in " + ctx.shown(dir));
    /* a use with no version yet, such as a venv without Rant, is installed too */
    std::vector<const Use*> bare;
    for (auto& u : uses)
        if (!u.version && changes(u, r)) bare.push_back(&u);
    if (adds.empty() && bare.empty()) {
        ctx.out.note(ctx.shown(dir) + " uses Rant already, move it to another release with `rant lib update`");
        return true;
    }
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "Rant " + r.version));
    bool ok = true, cmake = false, cargo = false;
    for (auto* u : bare) {
        try {
            report(ctx, u->dir, u->kind, bring(ctx, *u, r));
        } catch (const app::Failure& e) {
            ctx.out.warn(ctx.shown(u->dir) + " " + kind_name(u->kind) + ": " + e.what());
            ok = false;
        }
    }
    for (auto& [kind, file] : adds) {
        try {
            if (kind == Kind::Rust) require_rust(r);
            std::string hint = add(kind, file, r.version);
            if (kind == Kind::CSharp) install_nupkg(ctx, r);
            report(ctx, dir, kind, "added to " + ctx.shown(file));
            if (!hint.empty()) ctx.out.note("    link it to your target: " + hint);
            cmake |= kind == Kind::CMake;
            cargo |= kind == Kind::Rust;
        } catch (const app::Failure& e) {
            ctx.out.warn(ctx.shown(dir) + " " + kind_name(kind) + ": " + e.what());
            ok = false;
        }
    }
    if (cmake) ctx.out.note("CMake fetches it on the next configure, which `rant build` runs");
    if (cargo) ctx.out.note("Cargo fetches it on the next build, which `rant build` runs");
    return ok;
}

bool update(const app::Context& ctx, const fs::path& dir, const net::Release& r) {
    std::vector<Use> uses = under(dir);
    if (uses.empty()) throw app::Failure("nothing under " + ctx.shown(dir) + " uses Rant yet, add it with `rant lib install <folder>`");

    /* every package under a folder is a lot to change unasked, so the list comes first */
    ctx.out.line(ctx.out.paint(ui::Style::Bold, "Rant " + r.version));
    size_t n = 0;
    for (auto& u : uses)
        if (changes(u, r)) {
            report(ctx, u.dir, u.kind, (u.version ? *u.version : "none") + " -> " + r.version + " in " + ctx.shown(u.file));
            n++;
        }
    if (n == 0) {
        ctx.out.line("  " + ctx.out.paint(ui::Style::Dim, "everything here is at " + r.version));
    } else if (!ui::confirm("Update " + std::to_string(n) + (n == 1 ? " package?" : " packages?"), ctx.yes)) {
        ctx.out.note("nothing was changed");
        return false;
    }

    bool ok = true, cmake = false, cargo = false;
    size_t done = 0;
    for (auto& u : uses) {
        try {
            if (bring(ctx, u, r).empty()) continue;
            done++;
            cmake |= u.kind == Kind::CMake;
            cargo |= u.kind == Kind::Rust;
        } catch (const app::Failure& e) {
            ctx.out.warn(ctx.shown(u.dir) + " " + kind_name(u.kind) + ": " + e.what());
            ok = false;
        }
    }
    if (done) ctx.out.line("updated " + std::to_string(done) + (done == 1 ? " package" : " packages"));
    if (cmake) ctx.out.note("CMake fetches it on the next configure, which `rant build` runs");
    if (cargo) ctx.out.note("Cargo fetches it on the next build, which `rant build` runs");
    return ok;
}

}
