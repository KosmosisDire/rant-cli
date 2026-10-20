#include "packages/packages.hpp"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "net/github.hpp"
#include "process/command.hpp"
#include "state/state.hpp"
#include "util/home.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <winioctl.h>
#endif

namespace packages {

using json = nlohmann::json;

const char* const official = "KosmosisDire/rant-packages";

static const char* const package_file = "package.rant";

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

static std::string read_file(const fs::path& p) {
    std::ifstream in(p, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

fs::path dir() { return util::rant_home() / "packages"; }
fs::path sources_file() { return util::rant_home() / "sources.rant"; }

/* Where a fetched database is kept for completion, a name its repo cannot clash in. */
static fs::path cache_dir() { return util::rant_home() / "cache" / "databases"; }
static fs::path cached(const std::string& repo) {
    std::string file = repo;
    std::replace(file.begin(), file.end(), '/', '+');
    return cache_dir() / (file + ".rant");
}

static config::Database checked(config::Database db) {
    if (!db.diagnostics.empty()) throw app::Failure(db.diagnostics[0].str());
    return db;
}

Sources::Sources() {
    if (fs::is_regular_file(sources_file())) local_ = checked(config::read_database(sources_file(), read_file(sources_file()), true));
    databases_.push_back({ official, std::nullopt });
    for (auto& d : local_.databases)
        if (d != official) databases_.push_back({ d, std::nullopt });
}

std::vector<std::string> Sources::databases() const {
    std::vector<std::string> out;
    for (auto& [repo, _] : databases_) out.push_back(repo);
    return out;
}

const config::Database& Sources::fetched(size_t i) {
    auto& [repo, db] = databases_[i];
    if (db) return *db;
    std::optional<std::string> text = net::fetch(net::raw_url(repo, "HEAD", "packages.rant"));
    if (!text) throw app::Failure(repo + " holds no packages.rant, so it is no package database");
    db = checked(config::read_database(repo + "/packages.rant", *text, false));
    std::error_code ec;
    fs::create_directories(cache_dir(), ec);
    std::ofstream(cached(repo), std::ios::binary) << *text;
    return *db;
}

static Source source_of(const config::Listed& l, const std::string& named_by) { return { l.name, l.repo, l.path, named_by }; }

std::optional<Source> Sources::find(const std::string& name) {
    for (auto& l : local_.packages)
        if (l.name == name) return source_of(l, "sources.rant");
    for (size_t i = 0; i < databases_.size(); i++)
        for (auto& l : fetched(i).packages)
            if (l.name == name) return source_of(l, databases_[i].first);
    return std::nullopt;
}

std::vector<Source> Sources::all() {
    std::vector<Source> out;
    auto add = [&](const config::Listed& l, const std::string& by) {
        if (std::none_of(out.begin(), out.end(), [&](const Source& s) { return s.name == l.name; })) out.push_back(source_of(l, by));
    };
    for (auto& l : local_.packages) add(l, "sources.rant");
    for (size_t i = 0; i < databases_.size(); i++)
        for (auto& l : fetched(i).packages) add(l, databases_[i].first);
    return out;
}

std::vector<std::string> cached_names() {
    std::vector<std::string> out;
    std::error_code ec;
    std::vector<std::pair<fs::path, bool>> files = { { sources_file(), true } };
    for (fs::directory_iterator it(cache_dir(), ec), end; !ec && it != end; it.increment(ec)) files.push_back({ it->path(), false });
    for (auto& [file, local] : files) {
        if (!fs::is_regular_file(file, ec)) continue;
        for (auto& l : config::read_database(file, read_file(file), local).packages)
            if (std::find(out.begin(), out.end(), l.name) == out.end()) out.push_back(l.name);
    }
    return out;
}

/* Links and junctions. A package from a local build folder is a link to it, so a new build
 * counts at once, and removing a link must never reach into the folder. */

bool is_link(const fs::path& p) {
#ifdef _WIN32
    DWORD a = GetFileAttributesW(p.c_str());
    return a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_REPARSE_POINT);
#else
    std::error_code ec;
    return fs::is_symlink(p, ec);
#endif
}

static void unlink(const fs::path& p) {
#ifdef _WIN32
    bool ok = RemoveDirectoryW(p.c_str()) || DeleteFileW(p.c_str());
#else
    std::error_code ec;
    bool ok = fs::remove(p, ec);
#endif
    if (!ok) throw app::Failure("cannot remove the link " + u8(p));
}

#ifdef _WIN32
/* A junction, which unlike a symbolic link needs no privilege. Its reparse data holds the
 * target twice: the NT path it resolves and the path shown. */
static void make_link(const fs::path& link, const fs::path& target) {
    std::wstring shown = target.wstring(), nt = L"\\??\\" + shown;
    if (!CreateDirectoryW(link.c_str(), nullptr)) throw app::Failure("cannot create " + u8(link));
    HANDLE h = CreateFileW(link.c_str(), GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    bool ok = h != INVALID_HANDLE_VALUE;
    if (ok) {
        USHORT nt_bytes = (USHORT)(nt.size() * sizeof(wchar_t)), shown_bytes = (USHORT)(shown.size() * sizeof(wchar_t));
        USHORT data = (USHORT)(8 + nt_bytes + 2 + shown_bytes + 2);
        std::vector<char> buf(8 + data, 0);
        auto put = [&](size_t at, USHORT v) { std::memcpy(&buf[at], &v, sizeof v); };
        DWORD tag = IO_REPARSE_TAG_MOUNT_POINT;
        std::memcpy(&buf[0], &tag, sizeof tag);
        put(4, data);
        put(8, 0);
        put(10, nt_bytes);
        put(12, (USHORT)(nt_bytes + 2));
        put(14, shown_bytes);
        std::memcpy(&buf[16], nt.data(), nt_bytes);
        std::memcpy(&buf[16 + nt_bytes + 2], shown.data(), shown_bytes);
        DWORD got = 0;
        ok = DeviceIoControl(h, FSCTL_SET_REPARSE_POINT, buf.data(), (DWORD)buf.size(), nullptr, 0, &got, nullptr);
        CloseHandle(h);
    }
    if (!ok) {
        RemoveDirectoryW(link.c_str());
        throw app::Failure("cannot link " + u8(link) + " to " + u8(target));
    }
}
#else
static void make_link(const fs::path& link, const fs::path& target) {
    std::error_code ec;
    fs::create_directory_symlink(target, link, ec);
    if (ec) throw app::Failure("cannot link " + u8(link) + " to " + u8(target) + ": " + ec.message());
}
#endif

void unlink_all() {
    std::error_code ec;
    for (fs::directory_iterator it(dir(), ec), end; !ec && it != end; it.increment(ec))
        if (is_link(it->path())) unlink(it->path());
}

/* Records, a JSON file beside each package's folder, since a linked folder is the user's. */

static fs::path record_file(const std::string& name) { return dir() / (name + ".json"); }

std::optional<Record> record(const std::string& name) {
    json j = json::parse(read_file(record_file(name)), nullptr, false);
    if (!j.is_object()) return std::nullopt;
    Record r;
    r.name = name;
    r.repo = j.value("repo", "");
    r.path = config::from_utf8(j.value("path", ""));
    r.once = j.value("once", false);
    r.version = j.value("version", "");
    return r;
}

static void save(const Record& r) {
    json j = json::object();
    if (!r.repo.empty()) j["repo"] = r.repo;
    if (!r.path.empty()) j["path"] = u8(r.path);
    if (r.once) j["once"] = true;
    if (!r.version.empty()) j["version"] = r.version;
    std::ofstream(record_file(r.name), std::ios::binary) << j.dump(2) << "\n";
}

std::vector<std::string> installed() {
    std::vector<std::string> out;
    std::error_code ec;
    for (fs::directory_iterator it(dir(), ec), end; !ec && it != end; it.increment(ec)) {
        std::string name = u8(it->path().filename());
        if (name.empty() || name[0] == '.') continue;
        if (it->path().extension() == ".json") name = u8(it->path().stem());
        else if (!fs::is_directory(it->path(), ec)) continue;
        if (std::find(out.begin(), out.end(), name) == out.end()) out.push_back(name);
    }
    std::sort(out.begin(), out.end());
    return out;
}

/* Replacing a package's folder: the old one steps aside under a dot name the config skips,
 * and goes once nothing holds it. */

static std::string stamp() { return std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()); }

/* Removes what earlier replacements of this package could not, now that it may be free. */
static void sweep(const std::string& name) {
    std::string prefix = "." + name + ".old-";
    std::error_code ec;
    for (fs::directory_iterator it(dir(), ec), end; !ec && it != end; it.increment(ec))
        if (u8(it->path().filename()).rfind(prefix, 0) == 0 && !is_link(it->path())) fs::remove_all(it->path(), ec);
}

/* Moves the package's folder out of the way, or drops its link. */
static void retire(const std::string& name) {
    fs::path target = dir() / name;
    if (is_link(target)) return unlink(target);
    std::error_code ec;
    if (!fs::exists(target, ec)) return;
    fs::path old = dir() / ("." + name + ".old-" + stamp());
    fs::rename(target, old, ec);
    if (ec) throw app::Failure("cannot replace " + u8(target) + ": " + ec.message());
    fs::remove_all(old, ec);
}

/* The nodes any workspace started whose program or folder lies in this package's folder. */
static std::vector<std::string> running_from(const fs::path& folder) {
    std::vector<std::string> out;
    for (auto& d : state::remembered()) {
        state::Store store(d);
        state::prune(store.state());
        store.save();
        for (auto& i : store.state().instances) {
            bool from = util::holds(folder, config::from_utf8(i.cwd));
            for (auto& a : i.argv) from = from || util::holds(folder, config::from_utf8(a));
            if (from) out.push_back(i.name);
        }
    }
    return out;
}

/* A package's folder cannot change under its running nodes: Windows refuses to move a folder
 * a process works in, and elsewhere they would run what is gone. A link may change. */
static void refuse_running(const std::string& name) {
    if (is_link(dir() / name)) return;
    std::vector<std::string> running = running_from(dir() / name);
    if (running.empty()) return;
    std::string s;
    for (size_t i = 0; i < running.size(); i++) s += (i == 0 ? "" : i + 1 == running.size() ? " and " : ", ") + running[i];
    bool one = running.size() == 1;
    throw app::Failure(s + (one ? " runs" : " run") + " from " + name + ", stop " + (one ? "it" : "them") + " first with `rant stop`");
}

/* The system's tar, which opens .tar.gz on Windows 10 and later too. On Windows its own,
 * since an MSYS tar earlier on PATH reads C: as a remote host. */
static fs::path tar_program() {
#ifdef _WIN32
    fs::path system = util::env_path("SystemRoot") / "System32" / "tar.exe";
    if (fs::is_regular_file(system)) return system;
#endif
    if (auto p = process::find_program("tar", fs::temp_directory_path())) return *p;
    throw app::Failure("unpacking needs tar, which is not on PATH");
}

/* Unpacks an archive into the package's folder. It must hold package.rant at its top. */
static void unpack(const std::string& name, const fs::path& archive) {
    fs::path staging = dir() / ("." + name + ".new");
    std::error_code ec;
    fs::remove_all(staging, ec);
    fs::create_directories(staging, ec);
    if (process::run({ { u8(tar_program()), "-xzf", u8(archive), "-C", u8(staging) }, fs::temp_directory_path(), {} }) != 0) {
        fs::remove_all(staging, ec);
        throw app::Failure("cannot unpack " + u8(archive));
    }
    if (!fs::is_regular_file(staging / package_file)) {
        fs::remove_all(staging, ec);
        throw app::Failure(u8(archive) + " holds no package.rant at its top, so it is no package archive");
    }
    retire(name);
    fs::rename(staging, dir() / name, ec);
    if (ec) throw app::Failure("cannot place " + u8(dir() / name) + ": " + ec.message());
}

static bool is_archive(const fs::path& p) {
    std::string s = u8(p.filename());
    return s.size() > 7 && s.compare(s.size() - 7, 7, ".tar.gz") == 0;
}

/* A release of a repo: its package.rant at the release's tag names the file to fetch. */
static void install_release(app::Context& ctx, const Source& s, const std::string& version, Record& rec) {
    net::Release r = net::release(s.repo, version);
    std::optional<Record> had = record(s.name);
    std::error_code ec;
    if (had && had->repo == s.repo && !had->once && had->version == r.version && fs::is_directory(dir() / s.name, ec)) {
        ctx.out.note(s.name + " " + r.version + " is already installed");
        rec = *had;
        return;
    }
    refuse_running(s.name);
    std::optional<std::string> text = net::fetch(net::raw_url(s.repo, r.tag, package_file));
    if (!text) throw app::Failure(s.repo + " has no package.rant at " + r.tag + ", so that release is no package");
    config::PackageRelease rel = config::read_release(dir() / s.name, *text, r.version, net::platform());
    if (!rel.diagnostics.empty()) throw app::Failure(s.repo + " " + r.tag + ": " + rel.diagnostics[0].str());
    if (rel.archive.empty() && rel.vsix.empty())
        throw app::Failure(s.name + " " + r.version + " names no `archive` or `vsix` in its package.rant, so it has nothing to install");
    if (!rel.vsix.empty()) throw app::Failure(s.name + " is a VS Code extension, which this rant cannot install yet");
    const net::Asset* a = r.asset(rel.archive);
    if (!a) throw app::Failure("the " + s.name + " " + r.version + " release has no " + rel.archive + ", it is not built for " + net::platform());
    ctx.out.note("downloading " + rel.archive);
    fs::path file = util::rant_home() / "cache" / (rel.archive + "-" + stamp());
    struct Gone {
        fs::path p;
        ~Gone() { std::error_code e; fs::remove(p, e); }
    } gone{ file };
    net::download(a->url, file);
    unpack(s.name, file);
    rec.version = r.version;
    ctx.out.note("installed " + s.name + " " + r.version + " from " + s.repo);
}

/* A local build folder, linked so each new build counts at once, or an archive, unpacked. */
static void install_path(app::Context& ctx, const Source& s) {
    std::error_code ec;
    if (fs::is_directory(s.path, ec)) {
        fs::path file = s.path / package_file;
        if (!fs::is_regular_file(file, ec)) throw app::Failure(u8(s.path) + " holds no package.rant, so it is no package");
        config::PackageRelease rel = config::read_release(s.path, read_file(file), "", net::platform());
        if (!rel.diagnostics.empty()) throw app::Failure(rel.diagnostics[0].str());
        if (!rel.vsix.empty()) throw app::Failure(s.name + " is a VS Code extension, which this rant cannot install yet");
        retire(s.name);
        make_link(dir() / s.name, fs::absolute(s.path).lexically_normal());
        ctx.out.note("linked " + s.name + " to " + u8(s.path) + ", so its builds there count at once");
        return;
    }
    if (is_archive(s.path) && fs::is_regular_file(s.path, ec)) {
        unpack(s.name, s.path);
        ctx.out.note("installed " + s.name + " from " + u8(s.path));
        return;
    }
    if (!fs::exists(s.path, ec)) throw app::Failure("no folder or archive " + u8(s.path) + " for " + s.name);
    throw app::Failure(u8(s.path) + " is no package folder or .tar.gz archive");
}

void install(app::Context& ctx, const Source& s, const std::string& version) {
    std::error_code ec;
    fs::create_directories(dir(), ec);
    sweep(s.name);
    Record rec{ s.name, s.repo, s.path, s.named_by == "--from", "" };
    if (s.repo.empty()) {
        refuse_running(s.name);
        install_path(ctx, s);
    } else {
        install_release(ctx, s, version, rec);
    }
    save(rec);
}

void remove(app::Context& ctx, const std::string& name) {
    fs::path target = dir() / name;
    std::error_code ec;
    bool there = is_link(target) || fs::exists(target, ec);
    if (!there && !fs::exists(record_file(name), ec)) throw app::Failure(name + " is not installed, see `rant ls packages`");
    refuse_running(name);
    sweep(name);
    retire(name);
    fs::remove(record_file(name), ec);
    ctx.out.note("removed " + name);
}

}
