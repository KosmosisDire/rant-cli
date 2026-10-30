#include "packages/packages.hpp"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <fstream>
#include <iterator>
#include <system_error>
#include <thread>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "net/github.hpp"
#include "packages/vsix.hpp"
#include "process/command.hpp"
#include "services/services.hpp"
#include "state/state.hpp"
#include "util/home.hpp"
#include "util/match.hpp"

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

/* The names a list of sources gave when last read, kept for completion, in a file its key
 * cannot clash in. */
static fs::path names_file(const std::string& named_by) {
    std::string file = named_by;
    std::replace(file.begin(), file.end(), '/', '+');
    return util::rant_home() / "cache" / "names" / (file + ".json");
}

static config::Database checked(config::Database db) {
    if (!db.diagnostics.empty()) throw app::Failure(db.diagnostics[0].str());
    return db;
}

static std::optional<std::string> read_text(const fs::path& p) {
    std::error_code ec;
    if (!fs::is_regular_file(p, ec)) return std::nullopt;
    return read_file(p);
}

static bool ends_with(const std::string& s, const std::string& end) {
    return s.size() >= end.size() && s.compare(s.size() - end.size(), end.size(), end) == 0;
}

static bool is_archive(const fs::path& p) { return ends_with(u8(p.filename()), ".tar.gz"); }

/* GitHub takes a repo's owner and name in any case. */
static bool same_repo(const std::string& a, const std::string& b) {
    return std::equal(a.begin(), a.end(), b.begin(), b.end(), [](char x, char y) { return std::tolower((unsigned char)x) == std::tolower((unsigned char)y); });
}

static std::string shown(const Source& s) { return s.repo.empty() ? u8(s.path) : s.repo; }

/* A package.rant, which must name its package, read as dir would hold it. where names it in
 * errors. */
static config::PackageRelease named(const fs::path& dir, const std::string& where, const std::optional<std::string>& text) {
    if (!text) throw app::Failure(where + " holds no package.rant, so it is no package");
    config::PackageRelease rel = config::read_release(dir, *text);
    if (!rel.diagnostics.empty()) throw app::Failure(rel.diagnostics[0].str());
    if (rel.name.empty()) throw app::Failure("the package.rant of " + where + " gives no name, which a package needs to install");
    if (!config::is_package_name(rel.name))
        throw app::Failure(where + " names its package `" + rel.name + "`, which is no package name: lowercase letters, digits and _");
    return rel;
}

/* The package an archive holds by its file name, <name>-<version>-<platform>.tar.gz. */
static std::string archive_package(const fs::path& p) {
    std::string file = u8(p.filename());
    file.resize(file.size() - 7);
    return file.substr(0, file.find('-'));
}

Sources::Sources() {
    if (fs::is_regular_file(sources_file())) local_ = checked(config::read_database(sources_file(), read_file(sources_file()), true));
    lists_.push_back({ "sources.rant", std::nullopt });
    lists_.push_back({ official, std::nullopt });
    for (auto& d : local_.databases)
        if (!same_repo(d, official)) lists_.push_back({ d, std::nullopt });
}

std::vector<std::string> Sources::databases() const {
    std::vector<std::string> out;
    for (size_t i = 1; i < lists_.size(); i++) out.push_back(lists_[i].named_by);
    return out;
}

const std::vector<Sources::Found>& Sources::found(size_t i) {
    List& l = lists_[i];
    if (l.found) return *l.found;
    config::Database db = local_;
    if (i > 0) {
        std::optional<std::string> text = net::fetch(net::raw_url(l.named_by, "HEAD", "packages.rant"));
        if (!text) throw app::Failure(l.named_by + " holds no packages.rant, so it is no package database");
        db = checked(config::read_database(l.named_by + "/packages.rant", *text, false));
    }
    std::vector<Found> out;
    auto add = [&](Found f) {
        for (auto& o : out)
            if (o.source.name == f.source.name)
                return problems_.push_back(shown(f.source) + " also names the package " + f.source.name + ", after " + shown(o.source));
        out.push_back(std::move(f));
    };
    std::error_code ec;
    for (auto& p : db.paths) {
        try {
            if (fs::is_directory(p, ec)) {
                config::PackageRelease rel = named(p, u8(p), read_text(p / package_file));
                add({ { rel.name, "", p, l.named_by }, rel.description });
            } else if (is_archive(p) && fs::is_regular_file(p, ec)) {
                std::string name = archive_package(p);
                if (!config::is_package_name(name)) throw app::Failure(u8(p) + " is no archive named <name>-<version>-<platform>.tar.gz");
                add({ { name, "", p, l.named_by }, "" });
            } else {
                throw app::Failure(fs::exists(p, ec) ? u8(p) + " is no package folder or .tar.gz archive" : "no folder or archive " + u8(p));
            }
        } catch (const app::Failure& e) {
            problems_.push_back(e.what());
        }
    }
    std::vector<std::string> urls;
    for (auto& r : db.repos) urls.push_back(net::raw_url(r, "HEAD", package_file));
    std::vector<std::optional<std::string>> texts = net::fetch_all(urls);
    for (size_t k = 0; k < db.repos.size(); k++) {
        try {
            config::PackageRelease rel = named(config::from_utf8(db.repos[k]), db.repos[k], texts[k]);
            add({ { rel.name, db.repos[k], {}, l.named_by }, rel.description });
        } catch (const app::Failure& e) {
            problems_.push_back(e.what());
        }
    }
    json names = json::array();
    for (auto& f : out) names.push_back(f.source.name);
    fs::create_directories(names_file(l.named_by).parent_path(), ec);
    std::ofstream(names_file(l.named_by), std::ios::binary) << names.dump() << "\n";
    l.found = std::move(out);
    return *l.found;
}

std::optional<Source> Sources::find(const std::string& name) {
    for (size_t i = 0; i < lists_.size(); i++)
        for (auto& f : found(i))
            if (f.source.name == name) return f.source;
    return std::nullopt;
}

std::vector<Offer> Sources::offers(const std::string& pattern) {
    std::vector<Offer> out;
    for (size_t i = 0; i < lists_.size(); i++)
        for (auto& f : found(i))
            if (util::name_matches(pattern, f.source.name) &&
                std::none_of(out.begin(), out.end(), [&](const Offer& o) { return o.source.name == f.source.name; }))
                out.push_back({ f.source, f.description, record(f.source.name) });
    return out;
}

Source from_repo(const std::string& repo) {
    config::PackageRelease rel = named(config::from_utf8(repo), repo, net::fetch(net::raw_url(repo, "HEAD", package_file)));
    return { rel.name, repo, {}, "link" };
}

Carried carried(const net::Release& r, const std::string& name, const std::string& platform) {
    Carried c;
    std::string prefix = name + "-" + r.version;
    c.archive = r.asset(prefix + "-" + platform + ".tar.gz");
    c.extension = r.asset(prefix + ".vsix");
    for (auto& a : r.assets)
        if (a.name.rfind(prefix + "-", 0) == 0 && ends_with(a.name, ".tar.gz"))
            c.platforms.push_back(a.name.substr(prefix.size() + 1, a.name.size() - prefix.size() - 8));
    return c;
}

About about(const Source& s) {
    About a;
    std::error_code ec;
    if (s.repo.empty()) {
        if (fs::is_directory(s.path, ec)) a.description = named(s.path, u8(s.path), read_text(s.path / package_file)).description;
        return a;
    }
    net::Release r = net::release(s.repo);
    a.version = r.version;
    Carried c = carried(r, s.name, net::platform());
    for (const net::Asset* f : { c.archive, c.extension })
        if (f) a.files.push_back(f->name);
    std::string where = s.repo + " at " + r.tag;
    a.description = named(config::from_utf8(s.repo), where, net::fetch(net::raw_url(s.repo, r.tag, package_file))).description;
    return a;
}

std::vector<std::string> cached_names() {
    std::vector<std::string> lists = { "sources.rant", official }, out;
    std::error_code ec;
    if (fs::is_regular_file(sources_file(), ec))
        for (auto& d : config::read_database(sources_file(), read_file(sources_file()), true).databases) lists.push_back(d);
    for (auto& l : lists) {
        json j = json::parse(read_file(names_file(l)), nullptr, false);
        if (!j.is_array()) continue;
        for (auto& n : j)
            if (n.is_string() && std::find(out.begin(), out.end(), n.get<std::string>()) == out.end()) out.push_back(n.get<std::string>());
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
    r.named_by = j.value("named_by", "");
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
    if (!r.named_by.empty()) j["named_by"] = r.named_by;
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
/* Windows refuses to rename a folder while a file inside is open, as a virus scan holds fresh
 * files for a moment, so a refused rename is tried again for up to 3 s. */
static void rename_folder(const fs::path& from, const fs::path& to, std::error_code& ec) {
    auto until = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    for (;;) {
        fs::rename(from, to, ec);
#ifdef _WIN32
        bool held = ec.category() == std::system_category() &&
                    (ec.value() == ERROR_ACCESS_DENIED || ec.value() == ERROR_SHARING_VIOLATION);
#else
        bool held = false;
#endif
        if (!held || std::chrono::steady_clock::now() >= until) return;
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
}

static void retire(const std::string& name) {
    fs::path target = dir() / name;
    if (is_link(target)) return unlink(target);
    std::error_code ec;
    if (!fs::exists(target, ec)) return;
    fs::path old = dir() / ("." + name + ".old-" + stamp());
    rename_folder(target, old, ec);
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

/* Refuses a package.rant that names another package than the one being installed. */
static void refuse_other_name(const config::PackageRelease& rel, const std::string& name, const std::string& where) {
    if (rel.name != name) throw app::Failure(where + " holds the package " + rel.name + ", not " + name);
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
    if (!archive.empty()) {
        try {
            refuse_other_name(named(dir() / name, u8(archive), read_text(staging / package_file)), name, u8(archive));
        } catch (const app::Failure&) {
            fs::remove_all(staging, ec);
            throw;
        }
    }
    retire(name);
    rename_folder(staging, dir() / name, ec);
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

/* A release of a repo, the files it carries found by their names. */
static void install_release(app::Context& ctx, const Source& s, const std::string& version, Record& rec) {
    net::Release r = net::release(s.repo, version);
    std::optional<Record> had = record(s.name);
    std::error_code ec;
    bool retarget = ctx.args.has("editor") || ctx.args.has("profile");
    if (had && same_repo(had->repo, s.repo) && had->named_by != "--from" && had->version == r.version && fs::is_directory(dir() / s.name, ec) &&
        !(retarget && !had->extension.empty())) {
        ctx.out.note(s.name + " " + r.version + " is already installed");
        rec = *had;
        rec.named_by = s.named_by;
        return;
    }
    refuse_running(s.name);
    std::string where = s.repo + " at " + r.tag;
    std::optional<std::string> text = net::fetch(net::raw_url(s.repo, r.tag, package_file));
    refuse_other_name(named(config::from_utf8(s.repo), where, text), s.name, where);
    Carried c = carried(r, s.name, net::platform());
    const net::Asset *archive = c.archive, *extension = c.extension;
    std::string release = "the " + s.name + " " + r.version + " release";
    if (!archive && !c.platforms.empty()) {
        std::string built;
        for (auto& p : c.platforms) built += (built.empty() ? "" : ", ") + p;
        throw app::Failure(release + " is built for " + built + ", not for " + net::platform());
    }
    if (!archive && !extension)
        throw app::Failure(release + " carries no " + s.name + "-" + r.version + "-" + net::platform() + ".tar.gz or " + s.name + "-" +
                           r.version + ".vsix, so it has nothing to install");
    if (!extension && retarget) throw app::Failure(s.name + " is no VS Code extension, so --editor and --profile do not apply");
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

/* The newest extension of a package a build folder holds, which its build wrote last, named as
 * a release names it. Empty when there is none. */
static fs::path newest_vsix(const fs::path& folder, const std::string& name) {
    fs::path out;
    fs::file_time_type when{};
    std::error_code ec;
    for (fs::directory_iterator it(folder, ec), end; !ec && it != end; it.increment(ec)) {
        std::string file = u8(it->path().filename());
        if (file.rfind(name + "-", 0) == 0 && ends_with(file, ".vsix") && (out.empty() || it->last_write_time(ec) > when)) {
            out = it->path();
            when = it->last_write_time(ec);
        }
    }
    return out;
}

/* A local build folder, linked so each new build counts at once, or an archive, unpacked. */
static void install_path(app::Context& ctx, const Source& s, Record& rec) {
    std::error_code ec;
    bool retarget = ctx.args.has("editor") || ctx.args.has("profile");
    if (fs::is_directory(s.path, ec)) {
        refuse_other_name(named(s.path, u8(s.path), read_text(s.path / package_file)), s.name, u8(s.path));
        fs::path extension = newest_vsix(s.path, s.name);
        if (extension.empty() && retarget) throw app::Failure(s.name + " is no VS Code extension, so --editor and --profile do not apply");
        if (!extension.empty()) install_extension(ctx, extension_target(ctx, s.name), extension, rec);
        retire(s.name);
        make_link(dir() / s.name, fs::absolute(s.path).lexically_normal());
        ctx.out.note("linked " + s.name + " to " + u8(s.path) + (extension.empty() ? ", so its builds there count at once" : ""));
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
    /* a link names a package by its own word, so it never quietly takes the place of another */
    std::optional<Record> had = record(s.name);
    bool same = had && (s.repo.empty() ? had->repo.empty() && util::holds(had->path, s.path) && util::holds(s.path, had->path)
                                       : same_repo(had->repo, s.repo));
    if (had && !same && (had->named_by == "link" || s.named_by == "link"))
        throw app::Failure(s.name + " is installed from " + (had->repo.empty() ? u8(had->path) : had->repo) + ", `rant remove " + s.name +
                           "` first to install it from " + shown(s));
    sweep(s.name);
    Record rec{ s.name, s.repo, s.path, s.named_by, "" };
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
    if (rec->named_by == "--from") {
        ctx.out.note(name + " came from " + u8(rec->path) + " through --from, so update leaves it alone");
        return std::nullopt;
    }
    std::optional<Source> s = rec->named_by == "link" ? Source{ name, rec->repo, {}, "link" } : sources.find(name);
    if (!s) throw app::Failure("no source names " + name + " any more, see `rant ls sources`");
    Update u{ *s, rec->version, "", false };
    if (!s->repo.empty()) {
        net::Release r = net::release(s->repo);
        u.to = r.version;
        u.needed = !same_repo(rec->repo, s->repo) || net::newer(r.version, rec->version);
        return u;
    }
    if (!util::holds(rec->path, s->path) || !util::holds(s->path, rec->path)) {
        u.needed = true;
    } else if (fs::is_directory(s->path, ec)) {
        /* a linked folder is current by itself, but its extension is built into a file */
        u.needed = !is_link(target);
        if (!rec->extension.empty()) {
            fs::path extension = newest_vsix(s->path, name);
            if (extension.empty()) throw app::Failure(u8(s->path) + " holds no " + name + "-*.vsix to install, build one there first");
            u.to = config::read_vsix(extension).version;
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
