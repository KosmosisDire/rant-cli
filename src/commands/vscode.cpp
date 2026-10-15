#include <cctype>
#include <fstream>
#include <sstream>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/vscode.hpp"
#include "complete/complete.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "util/home.hpp"

namespace commands {

namespace fs = std::filesystem;

static const char* vscode_repo = "KosmosisDire/rant-vscode";
static const char* extension_id = "kosmosisdire.vscode-rant";

static std::string u8(const fs::path& p) { return config::to_utf8(p); }

namespace vscode {

/* Holds the editor rant installed the extension into, so update and uninstall leave an
 * extension someone else installed alone. */
static fs::path marker() { return util::rant_home() / "vscode.editor"; }

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

std::optional<fs::path> recorded_editor() {
    std::ifstream in(marker());
    std::string path;
    if (!std::getline(in, path) || path.empty()) return std::nullopt;
    fs::path p = config::from_utf8(path);
    if (!fs::is_regular_file(p)) return std::nullopt;
    return p;
}

std::string installed_version(const fs::path& editor) {
    std::optional<std::string> listing =
        process::output({ { u8(editor), "--list-extensions", "--show-versions" }, fs::temp_directory_path(), {} });
    return listing ? version_in(*listing) : std::string();
}

net::Release latest() { return net::release(vscode_repo); }

/* Installs a vsix through the editor, then remembers the editor. version is the release's, empty for a
 * built vsix, which is then read back: a running editor may list it a moment later. */
static void install_file(const app::Context& ctx, const fs::path& editor, const fs::path& vsix, std::string version = {}) {
    if (process::run({ { u8(editor), "--install-extension", u8(vsix), "--force" }, fs::temp_directory_path(), {} }) != 0)
        throw app::Failure(u8(editor) + " could not install " + u8(vsix));
    std::error_code ec;
    fs::create_directories(marker().parent_path(), ec);
    std::ofstream(marker()) << u8(editor) << "\n";
    if (version.empty()) version = installed_version(editor);
    ctx.out.note("installed the VS Code extension" + (version.empty() ? "" : " " + version) + " through " + u8(editor) +
                 ", reload the editor window to start it");
}

void install(const app::Context& ctx, const fs::path& editor, const net::Release& r) {
    std::string asset = "rant-vscode-" + r.version + ".vsix";
    const net::Asset* a = r.asset(asset);
    if (!a) throw app::Failure("the extension " + r.version + " release has no " + asset);
    ctx.out.note("downloading " + asset);
    struct Download {
        fs::path path;
        ~Download() { std::error_code ec; fs::remove(path, ec); }
    } vsix{ fs::temp_directory_path() / asset };
    net::download(a->url, vsix.path);
    install_file(ctx, editor, vsix.path, r.version);
}

void remove(const fs::path& editor) {
    if (process::run({ { u8(editor), "--uninstall-extension", extension_id }, fs::temp_directory_path(), {} }) != 0)
        throw app::Failure(u8(editor) + " could not remove the extension " + extension_id);
    std::error_code ec;
    fs::remove(marker(), ec);
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

static int install_(app::Context& ctx) {
    std::optional<fs::path> vsix;
    if (auto from = ctx.args.get("from")) {
        fs::path p = ctx.cwd() / config::from_utf8(*from);
        if (!fs::is_regular_file(p) || p.extension() != ".vsix") throw app::Failure("no file `" + *from + "`, give the path of a .vsix you built");
        vsix = p;
    }
    fs::path editor = editor_of(ctx);
    if (vsix) {
        vscode::install_file(ctx, editor, *vsix);
        return 0;
    }
    net::Release r;
    try {
        r = vscode::latest();
    } catch (const app::Failure& e) {
        throw app::Failure(std::string(e.what()) + ". To install an extension you built, run `rant vscode install --from PATH`");
    }
    vscode::install(ctx, editor, r);
    return 0;
}

static int uninstall_(app::Context& ctx) {
    std::optional<fs::path> recorded = ctx.args.has("editor") ? std::nullopt : vscode::recorded_editor();
    fs::path editor = recorded ? *recorded : editor_of(ctx);
    if (vscode::installed_version(editor).empty()) {
        ctx.out.note("the VS Code extension is not installed in " + u8(editor));
        return 0;
    }
    vscode::remove(editor);
    ctx.out.note("removed the VS Code extension from " + u8(editor));
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
                      { "editor", 0, "PROGRAM", "the editor's command line program, default `code`, such as `cursor`" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
