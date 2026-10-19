#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <sstream>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/vscode.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;
using json = nlohmann::json;

static const char* vscode_repo = "KosmosisDire/rant-vscode";
static const char* extension_id = "kosmosisdire.vscode-rant";

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

/* A string member of a JSON object, empty when it is missing or not a string. */
static std::string text(const json& o, const char* key) {
    json v = o.is_object() ? o.value(key, json()) : json();
    return v.is_string() ? v.get<std::string>() : std::string();
}

static json parse_file(const fs::path& p) {
    std::ifstream in(p, std::ios::binary);
    return json::parse(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>(), nullptr, false);
}

namespace vscode {

/* Holds the editor and profile rant installed the extension into, so update and uninstall
 * leave an extension someone else installed alone. */
static fs::path marker() { return util::rant_home() / "vscode.editor"; }

std::string shown(const Target& t) { return u8(t.editor) + (t.profile.empty() ? "" : " in the profile " + t.profile); }

std::string version_in(const std::string& listing) {
    const std::string prefix = std::string(extension_id) + "@";
    std::istringstream in(listing);
    for (std::string line; std::getline(in, line);) {
        while (!line.empty() && std::isspace((unsigned char)line.back())) line.pop_back();
        std::string id = line.substr(0, prefix.size());
        for (char& c : id) c = (char)std::tolower((unsigned char)c);
        if (id == prefix) return line.substr(prefix.size());
    }
    return {};
}

std::vector<Profile> profiles_in(const std::string& storage) {
    std::vector<Profile> out = { { "", "Default", false } };
    json j = json::parse(storage, nullptr, false);
    if (!j.is_object()) return out;
    for (auto& p : j.value("userDataProfiles", json::array())) {
        std::string id = text(p, "location"), name = text(p, "name");
        if (id.empty() || name.empty()) continue;
        json flags = p.value("useDefaultFlags", json());
        out.push_back({ id, name, flags.is_object() && flags.value("extensions", json()) == true });
    }
    return out;
}

/* The local path of a file:// URI, nullopt for any other scheme. */
static std::optional<fs::path> uri_path(const std::string& uri) {
    const std::string scheme = "file://";
    if (uri.compare(0, scheme.size(), scheme) != 0) return std::nullopt;
    std::string s;
    for (size_t i = scheme.size(); i < uri.size(); i++) {
        if (uri[i] == '%' && i + 2 < uri.size() && std::isxdigit((unsigned char)uri[i + 1]) && std::isxdigit((unsigned char)uri[i + 2])) {
            s += (char)std::stoi(uri.substr(i + 1, 2), nullptr, 16);
            i += 2;
        } else {
            s += uri[i];
        }
    }
    if (s.empty()) return std::nullopt;
    if (s[0] != '/') s = "//" + s;    /* file://server/share is a UNC path */
#ifdef _WIN32
    else if (s.size() > 2 && s[2] == ':') s.erase(0, 1);    /* file:///c:/x is c:/x */
#endif
    return config::from_utf8(s);
}

/* Whether dir is folder or inside it, without case on Windows. */
static bool holds(const fs::path& folder, const fs::path& dir) {
    auto key = [](const fs::path& p) {
        std::string s = config::to_utf8(p.lexically_normal());
#ifdef _WIN32
        for (char& c : s) c = c == '\\' ? '/' : (char)std::tolower((unsigned char)c);
#endif
        while (s.size() > 1 && s.back() == '/') s.pop_back();
        return s;
    };
    std::string f = key(folder), d = key(dir);
    if (f.empty() || d.compare(0, f.size(), f) != 0) return false;
    return d.size() == f.size() || d[f.size()] == '/' || f.back() == '/';
}

std::vector<std::string> folder_profiles(const std::string& storage, const fs::path& dir) {
    std::vector<std::string> out;
    json j = json::parse(storage, nullptr, false);
    json associations = j.is_object() ? j.value("profileAssociations", json()) : json();
    json folders = associations.is_object() ? associations.value("workspaces", json()) : json();
    if (!folders.is_object()) return out;
    for (auto& [uri, id] : folders.items()) {
        std::optional<fs::path> folder = uri_path(uri);
        if (!id.is_string() || !folder || !holds(*folder, dir)) continue;
        std::string p = id.get<std::string>();
        if (p == "__default__profile__") p.clear();
        if (std::find(out.begin(), out.end(), p) == out.end()) out.push_back(p);
    }
    return out;
}

/* Where the editor keeps its user data folder, as the editor itself picks it. Empty when the
 * environment names no home. */
static fs::path app_data() {
#ifdef _WIN32
    fs::path appdata = util::env_path("APPDATA");
    fs::path home = util::env_path("USERPROFILE");
    if (!appdata.empty() || home.empty()) return appdata;
    return home / "AppData" / "Roaming";
#elif defined(__APPLE__)
    fs::path home = util::home_dir();
    return home.empty() ? home : home / "Library" / "Application Support";
#else
    fs::path xdg = util::env_path("XDG_CONFIG_HOME");
    fs::path home = util::home_dir();
    if (!xdg.empty() || home.empty()) return xdg;
    return home / ".config";
#endif
}

/* The editor's storage.json, which lists its profiles. The product.json of the editor's install
 * names its user data folder, and a portable install keeps that in its own data folder. Empty
 * when there is no product.json, as for a remote terminal's `code`. */
static fs::path storage_of(const fs::path& editor) {
    std::error_code ec;
    fs::path root = fs::canonical(editor, ec).parent_path().parent_path();
    if (ec) return {};
    /* app/ on macOS, resources/app/ on Linux, <version>/resources/app/ on Windows */
    std::vector<fs::path> where = { root / "product.json", root / "resources" / "app" / "product.json" };
    for (fs::directory_iterator it(root, ec), end; !ec && it != end; it.increment(ec))
        where.push_back(it->path() / "resources" / "app" / "product.json");
    json product;
    for (auto& p : where)
        if (fs::is_regular_file(p, ec)) {
            product = parse_file(p);
            break;
        }
    std::string name = text(product, "nameShort");
    if (name.empty()) return {};
    fs::path portable = util::env_path("VSCODE_PORTABLE");
    if (portable.empty()) {
#ifdef __APPLE__
        std::string own = text(product, "portable");
        portable = root.parent_path().parent_path().parent_path().parent_path() /
                   (own.empty() ? text(product, "applicationName") + "-portable-data" : own);
#else
        portable = root / "data";
#endif
    }
    fs::path data;
    if (!product.contains("target") && fs::is_directory(portable, ec)) data = portable / "user-data";
    else if (fs::path own = util::env_path("VSCODE_APPDATA"); !own.empty()) data = own / name;
    else if (fs::path base = app_data(); !base.empty()) data = base / name;
    else return {};
    return data / "User" / "globalStorage" / "storage.json";
}

std::optional<Target> recorded() {
    std::ifstream in(marker());
    std::string path, profile;
    if (!std::getline(in, path) || path.empty()) return std::nullopt;
    std::getline(in, profile);
    fs::path p = config::from_utf8(path);
    if (!fs::is_regular_file(p)) return std::nullopt;
    return Target{ p, profile };
}

/* The editor's command line with these arguments, acting on the target's profile. */
static process::Command command(const Target& t, std::vector<std::string> args) {
    args.insert(args.begin(), u8(t.editor));
    if (!t.profile.empty()) {
        args.push_back("--profile");
        args.push_back(t.profile);
    }
    return { args, fs::temp_directory_path(), {} };
}

std::string installed_version(const Target& t) {
    std::optional<std::string> listing = process::output(command(t, { "--list-extensions", "--show-versions" }));
    return listing ? version_in(*listing) : std::string();
}

net::Release latest() { return net::release(vscode_repo); }

/* Installs a vsix through the editor, then remembers the target. version is the release's, empty for a
 * built vsix, which is then read back: a running editor may list it a moment later. */
static void install_file(const app::Context& ctx, const Target& t, const fs::path& vsix, std::string version = {}) {
    if (process::run(command(t, { "--install-extension", u8(vsix), "--force" })) != 0)
        throw app::Failure(u8(t.editor) + " could not install " + u8(vsix));
    std::error_code ec;
    fs::create_directories(marker().parent_path(), ec);
    std::ofstream(marker()) << u8(t.editor) << "\n" << t.profile << "\n";
    if (version.empty()) version = installed_version(t);
    ctx.out.note("installed the VS Code extension" + (version.empty() ? "" : " " + version) + " through " + shown(t) +
                 ", reload the editor window to start it");
}

void install(const app::Context& ctx, const Target& t, const net::Release& r) {
    std::string asset = "rant-vscode-" + r.version + ".vsix";
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("the extension " + r.version + " release has no " + asset);
    ctx.out.note("downloading " + asset);
    struct Download {
        fs::path path;
        ~Download() { std::error_code ec; fs::remove(path, ec); }
    } vsix{ fs::temp_directory_path() / asset };
    net::download(a->url, vsix.path);
    install_file(ctx, t, vsix.path, r.version);
}

void remove(const Target& t) {
    if (process::run(command(t, { "--uninstall-extension", extension_id })) != 0)
        throw app::Failure(u8(t.editor) + " could not remove the extension " + extension_id);
    std::optional<Target> r = recorded();
    std::error_code ec;
    if (r && r->editor == t.editor && r->profile == t.profile) fs::remove(marker(), ec);
}

/* The name to install into for a profile, empty for one that holds the default's extensions. */
static std::string install_name(const Profile& p) { return p.id.empty() || p.shared ? std::string() : p.name; }

/* Whether this runs in a terminal of a VS Code window, or of an editor built from it. */
static bool in_editor_terminal() {
    const char* v = std::getenv("TERM_PROGRAM");
    return v && std::string(v) == "vscode";
}

/* --profile, else the profile this VS Code terminal's folder opens in, else the default when
 * no other profile has its own extensions, else the one the user picks. */
static std::string profile_of(const app::Context& ctx, const fs::path& editor) {
    if (auto given = ctx.args.get("profile")) return *given;
    fs::path storage_file = storage_of(editor);
    std::string storage;
    if (!storage_file.empty()) {
        std::ifstream in(storage_file, std::ios::binary);
        storage.assign(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    std::vector<Profile> all = profiles_in(storage);
    std::vector<std::string> ids = in_editor_terminal() ? folder_profiles(storage, ctx.cwd()) : std::vector<std::string>();
    if (ids.size() == 1)
        for (auto& p : all)
            if (p.id == ids[0]) return install_name(p);
    std::vector<Profile> own;
    for (auto& p : all)
        if (!p.shared) own.push_back(p);
    if (own.size() == 1) return {};
    ctx.out.note(u8(editor) + " has the profiles:");
    size_t width = std::to_string(own.size()).size();
    for (size_t i = 0; i < own.size(); i++) {
        std::string n = std::to_string(i + 1);
        ctx.out.note("  " + std::string(width - n.size(), ' ') + n + "  " + own[i].name);
    }
    for (;;) {
        std::optional<std::string> answer = ui::ask("Install into which profile? Give its number or name:");
        if (!answer) throw app::Failure("no profile picked, pass --profile NAME with one of the names above");
        for (size_t i = 0; i < own.size(); i++)
            if (*answer == std::to_string(i + 1) || *answer == own[i].name) return install_name(own[i]);
        ctx.out.note("no profile `" + *answer + "`");
    }
}

}

/* The editor's command line program: --editor when given, else VS Code's `code`. */
static fs::path editor_of(const app::Context& ctx) {
    std::optional<std::string> given = ctx.args.get("editor");
    if (given) {
        if (auto p = process::find_program(*given, ctx.cwd())) return *p;
        throw app::Failure("no editor `" + *given + "` found, give the program the editor installs extensions with");
    }
    if (auto p = process::find_program("code", ctx.cwd())) return *p;
    throw app::Failure("VS Code's `code` command is not on PATH. In VS Code run `Shell Command: Install 'code' command in PATH`, "
                       "or pass --editor for another editor");
}

static vscode::Target target_of(const app::Context& ctx) {
    fs::path editor = editor_of(ctx);
    return { editor, vscode::profile_of(ctx, editor) };
}

static int install_(app::Context& ctx) {
    std::optional<fs::path> vsix;
    if (auto from = ctx.args.get("from")) {
        fs::path p = ctx.cwd() / config::from_utf8(*from);
        if (!fs::is_regular_file(p) || p.extension() != ".vsix") throw app::Failure("no file `" + *from + "`, give the path of a .vsix you built");
        vsix = p;
    }
    vscode::Target t = target_of(ctx);
    if (vsix) {
        vscode::install_file(ctx, t, *vsix);
        return 0;
    }
    net::Release r;
    try {
        r = vscode::latest();
    } catch (const app::Failure& e) {
        throw app::Failure(std::string(e.what()) + ". To install an extension you built, run `rant vscode install --from PATH`");
    }
    vscode::install(ctx, t, r);
    return 0;
}

static int uninstall_(app::Context& ctx) {
    bool chosen = ctx.args.has("editor") || ctx.args.has("profile");
    std::optional<vscode::Target> recorded = chosen ? std::nullopt : vscode::recorded();
    vscode::Target t = recorded ? *recorded : target_of(ctx);
    if (vscode::installed_version(t).empty()) {
        ctx.out.note("the VS Code extension is not installed in " + vscode::shown(t));
        return 0;
    }
    vscode::remove(t);
    ctx.out.note("removed the VS Code extension from " + vscode::shown(t));
    return 0;
}

struct Sub {
    const char* name;
    int       (*run)(app::Context&);
};

static const std::vector<Sub>& subs() {
    static const std::vector<Sub> list = { { "install", install_ }, { "uninstall", uninstall_ } };
    return list;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) throw app::UsageError("vscode needs a command, see `rant vscode --help`");
    if (w.size() > 1) throw app::UsageError("vscode " + w[0] + " takes no arguments");
    for (auto& s : subs()) {
        if (w[0] != s.name) continue;
        if (w[0] != "install" && ctx.args.has("from")) throw app::UsageError("--from goes with `rant vscode install`");
        return s.run(ctx);
    }
    throw app::UsageError("unknown vscode command `" + w[0] + "`, see `rant vscode --help`");
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    std::vector<std::string> out;
    for (auto& s : subs()) out.emplace_back(s.name);
    return { out };
}

app::Command vscode_() {
    app::Command c{ "vscode", "install | uninstall",
                    "install the VS Code extension through the editor's `code` command, in a remote terminal onto that remote",
                    app::Section::Setup,
                    { { "from", 0, "PATH", "with install: install this .vsix instead of downloading the newest release" },
                      { "editor", 0, "PROGRAM", "the editor's command line program, default `code`, such as `cursor`" },
                      { "profile", 0, "NAME", "the editor profile, default the one this VS Code terminal's folder opens in, else asked" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
