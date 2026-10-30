#include "packages/vsix.hpp"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <optional>
#include <sstream>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "process/command.hpp"
#include "ui/prompt.hpp"
#include "util/home.hpp"

namespace packages::vsix {

namespace fs = std::filesystem;
using json = nlohmann::json;

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

std::string shown(const Target& t) { return u8(t.editor) + (t.profile.empty() ? "" : " in the profile " + t.profile); }

std::string version_in(const std::string& listing, const std::string& id) {
    std::string prefix = id + "@";
    for (char& c : prefix) c = (char)std::tolower((unsigned char)c);
    std::istringstream in(listing);
    for (std::string line; std::getline(in, line);) {
        while (!line.empty() && std::isspace((unsigned char)line.back())) line.pop_back();
        std::string head = line.substr(0, prefix.size());
        for (char& c : head) c = (char)std::tolower((unsigned char)c);
        if (head == prefix) return line.substr(prefix.size());
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

std::vector<std::string> folder_profiles(const std::string& storage, const fs::path& dir) {
    std::vector<std::string> out;
    json j = json::parse(storage, nullptr, false);
    json associations = j.is_object() ? j.value("profileAssociations", json()) : json();
    json folders = associations.is_object() ? associations.value("workspaces", json()) : json();
    if (!folders.is_object()) return out;
    for (auto& [uri, id] : folders.items()) {
        std::optional<fs::path> folder = uri_path(uri);
        if (!id.is_string() || !folder || !util::holds(*folder, dir)) continue;
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

/* The editor's command line with these arguments, acting on the target's profile. */
static process::Command command(const Target& t, std::vector<std::string> args) {
    args.insert(args.begin(), u8(t.editor));
    if (!t.profile.empty()) {
        args.push_back("--profile");
        args.push_back(t.profile);
    }
    return { args, fs::temp_directory_path(), {} };
}

std::string installed_version(const Target& t, const std::string& id) {
    std::optional<std::string> listing = process::output(command(t, { "--list-extensions", "--show-versions" }));
    return listing ? version_in(*listing, id) : std::string();
}

config::Vsix install(const Target& t, const fs::path& file) {
    config::Vsix v = config::read_vsix(file);
    if (!v.diagnostics.empty()) throw app::Failure(v.diagnostics[0].message);
    if (process::run(command(t, { "--install-extension", u8(file), "--force" })) != 0)
        throw app::Failure(u8(t.editor) + " could not install " + u8(file));
    return v;
}

void remove(const Target& t, const std::string& id) {
    if (process::run(command(t, { "--uninstall-extension", id })) != 0)
        throw app::Failure(u8(t.editor) + " could not remove the extension " + id);
}

/* The name to install into for a profile, empty for one that holds the default's extensions. */
static std::string install_name(const Profile& p) { return p.id.empty() || p.shared ? std::string() : p.name; }

/* Whether this runs in a terminal of a VS Code window, or of an editor built from it. */
static bool in_editor_terminal() {
    const char* v = std::getenv("TERM_PROGRAM");
    return v && std::string(v) == "vscode";
}

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

bool editor_found(const app::Context& ctx) {
    std::optional<std::string> given = ctx.args.get("editor");
    return process::find_program(given ? *given : "code", ctx.cwd()).has_value();
}

Target target(const app::Context& ctx) {
    fs::path editor = editor_of(ctx);
    return { editor, profile_of(ctx, editor) };
}

}
