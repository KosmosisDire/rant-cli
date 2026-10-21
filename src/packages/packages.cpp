#include "packages/packages.hpp"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <fstream>
#include <iterator>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "net/github.hpp"
#include "packages/vsix.hpp"
#include "process/command.hpp"
#include "services/services.hpp"
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

About about(const Source& s, bool release) {
    About a;
    std::string text;
    std::error_code ec;
    if (!s.repo.empty()) {
        std::string ref = "HEAD";
        if (release) {
            net::Release r = net::release(s.repo);
            a.version = r.version;
            ref = r.tag;
        }
        std::optional<std::string> t = net::fetch(net::raw_url(s.repo, ref, package_file));
        if (!t) throw app::Failure(s.repo + " has no package.rant at " + ref);
        text = *t;
    } else if (fs::is_regular_file(s.path / package_file, ec)) {
        text = read_file(s.path / package_file);
    } else {
        return a;
    }
    config::PackageRelease rel = config::read_release(dir() / s.name, text, a.version, net::platform());
    if (!rel.diagnostics.empty()) throw app::Failure(rel.diagnostics[0].str());
    a.description = rel.description;
    /* a file name needs the release's version, and a path installs no file */
    if (release && !s.repo.empty()) {
        a.archive = rel.archive;
        a.vsix = rel.vsix;
    }
    return a;
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
    json e = j.value("extension", json::object());
    if (e.is_object()) {
        r.extension = e.value("id", "");
        r.editor = config::from_utf8(e.value("editor", ""));
        r.profile = e.value("profile", "");
    }
    return r;
}

static void save(const Record& r) {
    json j = json::object();
    if (!r.repo.empty()) j["repo"] = r.repo;
    if (!r.path.empty()) j["path"] = u8(r.path);
    if (r.once) j["once"] = true;
    if (!r.version.empty()) j["version"] = r.version;
    if (!r.extension.empty()) j["extension"] = { { "id", r.extension }, { "editor", u8(r.editor) }, { "profile", r.profile } };
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
        state::prune(store.state(), services::running_in);
        store.save();
        for (auto& i : store.state().instances)
            if (i.runs_from(folder)) out.push_back(i.name);
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

/* Fills a new folder for the package, an archive unpacked or only the package.rant text of a
 * release without one, then puts it in place of the old. */
static void place(const std::string& name, const fs::path& archive, const std::string& text = "") {
    fs::path staging = dir() / ("." + name + ".new");
    std::error_code ec;
    fs::remove_all(staging, ec);
    fs::create_directories(staging, ec);
    if (archive.empty()) {
        std::ofstream(staging / package_file, std::ios::binary) << text;
    } else if (process::run({ { u8(tar_program()), "-xzf", u8(archive), "-C", u8(staging) }, fs::temp_directory_path(), {} }) != 0) {
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

/* Where an extension goes: the editor and profile given, else those it went into before while
 * that editor is there, else the ones vsix::target finds, which may ask. */
static vsix::Target extension_target(const app::Context& ctx, const std::string& name) {
    std::optional<Record> had = record(name);
    std::error_code ec;
    if (!ctx.args.has("editor") && !ctx.args.has("profile") && had && !had->extension.empty() && fs::is_regular_file(had->editor, ec))
        return { had->editor, had->profile };
    return vsix::target(ctx);
}

/* Installs an extension into the target and records where. From a path the extension's own
 * version stands for the package's. */
static void install_extension(app::Context& ctx, const vsix::Target& t, const fs::path& file, Record& rec) {
    std::optional<Record> had = record(rec.name);
    config::Vsix v = vsix::install(t, file);
    rec.extension = v.id;
    rec.editor = t.editor;
    rec.profile = t.profile;
    if (rec.repo.empty()) rec.version = v.version;
    ctx.out.note("installed the extension " + v.id + " " + v.version + " through " + vsix::shown(t) + ", reload the editor window to start it");
    if (had && !had->extension.empty() && (had->editor != t.editor || had->profile != t.profile))
        ctx.out.note("it stays in " + vsix::shown({ had->editor, had->profile }) + " as well, where rant no longer updates it");
}

/* A file the downloads put in rant's cache, gone with this. */
struct Download {
    fs::path path;
    ~Download() {
        std::error_code ec;
        fs::remove(path, ec);
    }
};

static bool is_archive(const fs::path& p) {
    std::string s = u8(p.filename());
    return s.size() > 7 && s.compare(s.size() - 7, 7, ".tar.gz") == 0;
}

/* A release of a repo: its package.rant at the release's tag names the file to fetch. */
static void install_release(app::Context& ctx, const Source& s, const std::string& version, Record& rec) {
    net::Release r = net::release(s.repo, version);
    std::optional<Record> had = record(s.name);
    std::error_code ec;
    bool retarget = ctx.args.has("editor") || ctx.args.has("profile");
    if (had && had->repo == s.repo && !had->once && had->version == r.version && fs::is_directory(dir() / s.name, ec) &&
        !(retarget && !had->extension.empty())) {
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
    if (rel.vsix.empty() && retarget) throw app::Failure(s.name + " is no VS Code extension, so --editor and --profile do not apply");
    const net::Asset* archive = rel.archive.empty() ? nullptr : r.asset(rel.archive);
    const net::Asset* extension = rel.vsix.empty() ? nullptr : r.asset(rel.vsix);
    for (auto [name, asset] : { std::pair{ rel.archive, archive }, std::pair{ rel.vsix, extension } })
        if (!name.empty() && !asset) throw app::Failure("the " + s.name + " " + r.version + " release has no " + name + ", built for " + net::platform());
    /* the editor and profile first, since picking one may ask */
    std::optional<vsix::Target> t;
    if (extension) t = extension_target(ctx, s.name);
    std::vector<Download> files;
    for (auto* a : { archive, extension }) {
        if (!a) continue;
        ctx.out.note("downloading " + a->name);
        files.push_back({ util::rant_home() / "cache" / (stamp() + "-" + a->name) });
        net::download(a->url, files.back().path);
    }
    if (extension) install_extension(ctx, *t, files.back().path, rec);
    place(s.name, archive ? files.front().path : fs::path(), *text);
    rec.version = r.version;
    ctx.out.note("installed " + s.name + " " + r.version + " from " + s.repo);
}

/* The newest .vsix in a build folder, which its build wrote last. */
static fs::path newest_vsix(const fs::path& folder) {
    fs::path out;
    fs::file_time_type when{};
    std::error_code ec;
    for (fs::directory_iterator it(folder, ec), end; !ec && it != end; it.increment(ec))
        if (it->path().extension() == ".vsix" && (out.empty() || it->last_write_time(ec) > when)) {
            out = it->path();
            when = it->last_write_time(ec);
        }
    if (out.empty()) throw app::Failure(u8(folder) + " holds no .vsix to install, build one there first");
    return out;
}

/* A local build folder, linked so each new build counts at once, or an archive, unpacked. */
static void install_path(app::Context& ctx, const Source& s, Record& rec) {
    std::error_code ec;
    bool retarget = ctx.args.has("editor") || ctx.args.has("profile");
    if (fs::is_directory(s.path, ec)) {
        fs::path file = s.path / package_file;
        if (!fs::is_regular_file(file, ec)) throw app::Failure(u8(s.path) + " holds no package.rant, so it is no package");
        config::PackageRelease rel = config::read_release(s.path, read_file(file), "", net::platform());
        if (!rel.diagnostics.empty()) throw app::Failure(rel.diagnostics[0].str());
        if (rel.vsix.empty() && retarget) throw app::Failure(s.name + " is no VS Code extension, so --editor and --profile do not apply");
        if (!rel.vsix.empty()) install_extension(ctx, extension_target(ctx, s.name), newest_vsix(s.path), rec);
        retire(s.name);
        make_link(dir() / s.name, fs::absolute(s.path).lexically_normal());
        ctx.out.note("linked " + s.name + " to " + u8(s.path) + (rel.vsix.empty() ? ", so its builds there count at once" : ""));
        return;
    }
    if (retarget) throw app::Failure("--editor and --profile go with a VS Code extension, which an archive holds none of");
    if (is_archive(s.path) && fs::is_regular_file(s.path, ec)) {
        place(s.name, s.path);
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
        install_path(ctx, s, rec);
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
    std::optional<Record> rec = record(name);
    if (rec && !rec->extension.empty()) {
        vsix::Target t{ rec->editor, rec->profile };
        if (!fs::is_regular_file(t.editor, ec))
            ctx.out.warn(u8(t.editor) + " is gone, so the extension " + rec->extension + " stays wherever it went");
        else if (!vsix::installed_version(t, rec->extension).empty())
            vsix::remove(t, rec->extension);
    }
    sweep(name);
    retire(name);
    fs::remove(record_file(name), ec);
    ctx.out.note("removed " + name);
}

std::optional<Update> check(app::Context& ctx, Sources& sources, const std::string& name) {
    std::optional<Record> rec = record(name);
    fs::path target = dir() / name;
    std::error_code ec;
    if (!rec) {
        if (!is_link(target) && !fs::exists(target, ec)) throw app::Failure(name + " is not installed, `rant install " + name + "` installs it");
        ctx.out.note(name + " was put in " + u8(dir()) + " by hand, so update leaves it alone");
        return std::nullopt;
    }
    if (rec->once) {
        ctx.out.note(name + " came from " + u8(rec->path) + " through --from, so update leaves it alone");
        return std::nullopt;
    }
    std::optional<Source> s = sources.find(name);
    if (!s) throw app::Failure("no source names " + name + " any more, see `rant ls sources`");
    Update u{ *s, rec->version, "", false };
    if (!s->repo.empty()) {
        net::Release r = net::release(s->repo);
        u.to = r.version;
        u.needed = rec->repo != s->repo || net::newer(r.version, rec->version);
        return u;
    }
    if (!util::holds(rec->path, s->path) || !util::holds(s->path, rec->path)) {
        u.needed = true;
    } else if (fs::is_directory(s->path, ec)) {
        /* a linked folder is current by itself, but its extension is built into a file */
        u.needed = !is_link(target);
        if (!rec->extension.empty()) {
            u.to = config::read_vsix(newest_vsix(s->path)).version;
            u.needed = u.needed || u.to != rec->version;
        }
    } else {
        u.needed = fs::last_write_time(s->path, ec) > fs::last_write_time(record_file(name), ec);
    }
    return u;
}

std::optional<config::NodeType> installed_node(const std::string& package, const std::string& node) {
    config::Opened o = config::open(util::rant_home(), true);
    if (!o.workspace) return std::nullopt;
    for (auto& p : o.workspace->packages)
        if (p.installed && p.name == package)
            for (auto& n : p.nodes)
                if (n.name == node) return n;
    return std::nullopt;
}

}
