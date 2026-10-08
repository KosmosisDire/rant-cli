#include "scaffold/scaffold.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <iterator>
#include <utility>

#include <inja/inja.hpp>

#include "app/failure.hpp"
#include "scaffold/builtin.hpp"

namespace scaffold {

using json = nlohmann::json;
using Files = std::vector<std::pair<std::string, std::string>>;    /* path with / and bytes */

static const std::string manifest_name = "template.hcl";

static std::vector<std::string> words(const std::string& s) {
    std::vector<std::string> out;
    std::string cur;
    bool prev_lower = false;
    for (char ch : s) {
        unsigned char c = (unsigned char)ch;
        if (!std::isalnum(c)) {
            if (!cur.empty()) out.push_back(std::move(cur));
            cur.clear();
            prev_lower = false;
            continue;
        }
        if (std::isupper(c) && prev_lower && !cur.empty()) {
            out.push_back(std::move(cur));
            cur.clear();
        }
        prev_lower = std::islower(c) || std::isdigit(c);
        cur += (char)std::tolower(c);
    }
    if (!cur.empty()) out.push_back(cur);
    return out;
}

std::string in_case(const std::string& name, const std::string& which) {
    std::vector<std::string> w = words(name);
    std::string out;
    for (size_t i = 0; i < w.size(); i++) {
        std::string word = w[i];
        if (which == "snake" || which == "kebab") {
            if (i) out += which == "snake" ? "_" : "-";
        } else if (which == "pascal" || i > 0) {
            word[0] = (char)std::toupper((unsigned char)word[0]);
        }
        out += word;
    }
    return out;
}

static std::string read_file(const fs::path& p) {
    std::ifstream in(p, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

static Files files_of(const Origin& o) {
    Files out;
    if (!o.builtin.empty()) {
        std::string prefix = o.builtin + "/";
        for (auto& f : builtin_files()) {
            std::string path = f.path;
            if (path.rfind(prefix, 0) == 0) out.push_back({ path.substr(prefix.size()), std::string((const char*)f.data, f.size) });
        }
        if (out.empty()) throw app::Failure("no built in template `" + o.builtin + "`");
        return out;
    }
    std::error_code ec;
    for (auto it = fs::recursive_directory_iterator(o.dir, ec); it != fs::recursive_directory_iterator(); it.increment(ec)) {
        if (ec) break;
        if (it->path().filename() == ".git") {
            it.disable_recursion_pending();
            continue;
        }
        if (it->is_regular_file(ec)) out.push_back({ fs::relative(it->path(), o.dir).generic_u8string(), read_file(it->path()) });
    }
    std::sort(out.begin(), out.end());
    return out;
}

/* The template.hcl of a template, and the path its errors name. */
static std::pair<std::string, fs::path> manifest_of(const Origin& o, const Files& files) {
    fs::path where = o.builtin.empty() ? o.dir / manifest_name : fs::path(o.builtin) / manifest_name;
    for (auto& [path, bytes] : files)
        if (path == manifest_name) return { bytes, where };
    throw app::Failure(config::to_utf8(o.dir) + " has no " + manifest_name + ", so it is not a template");
}

config::TemplateManifest describe(const Origin& origin) {
    Files files = files_of(origin);
    auto [text, where] = manifest_of(origin, files);
    return config::read_template(text, where, {}, false);
}

static void require(const config::TemplateManifest& m) {
    if (m.diagnostics.empty()) return;
    std::string why;
    for (auto& d : m.diagnostics) why += (why.empty() ? "" : "\n") + d.str();
    throw app::Failure(why);
}

static inja::Environment environment() {
    inja::Environment env;
    env.set_line_statement("\x01");    /* no line statements, a line of a file may well start with ## */
    for (const char* which : { "snake", "kebab", "pascal", "camel" })
        env.add_callback(which, 1, [which](inja::Arguments& args) { return in_case(args.at(0)->get<std::string>(), which); });
    return env;
}

static std::string render(inja::Environment& env, const std::string& text, const json& data, const std::string& what) {
    try {
        return env.render(text, data);
    } catch (const std::exception& e) {
        throw app::Failure(what + ": " + e.what());
    }
}

/* Every file a template and the templates it includes make, rendered, relative to where
 * they go. */
static void render_into(inja::Environment& env, const Origin& o, const json& data, Files& out, int depth) {
    if (depth > 8) throw app::Failure("templates include each other in a loop");
    Files files = files_of(o);
    auto [text, where] = manifest_of(o, files);
    config::TemplateManifest m = config::read_template(text, where, {}, false);
    require(m);
    for (auto& [path, bytes] : files) {
        if (path == manifest_name) continue;
        bool binary = bytes.find('\0') != std::string::npos;    /* copied as it is */
        out.push_back({ render(env, path, data, path), binary ? bytes : render(env, bytes, data, path) });
    }
    for (auto& inc : m.includes) render_into(env, Origin{ inc, {} }, data, out, depth + 1);
}

Made make(const Origin& origin, const fs::path& dest, const std::string& name, const std::vector<std::string>& params) {
    Files files = files_of(origin);
    auto [text, where] = manifest_of(origin, files);
    config::TemplateManifest m = config::read_template(text, where, params, true);
    require(m);
    json data = json::parse(m.values);
    data["name"] = name;

    inja::Environment env = environment();
    Files rendered;
    render_into(env, origin, data, rendered, 0);
    for (auto& [rel, _] : rendered)
        if (fs::exists(dest / fs::u8path(rel))) throw app::Failure(config::to_utf8(dest / fs::u8path(rel)) + " exists already, nothing was written");

    Made made;
    for (auto& [rel, bytes] : rendered) {
        fs::path p = dest / fs::u8path(rel);
        std::error_code ec;
        fs::create_directories(p.parent_path(), ec);
        std::ofstream out(p, std::ios::binary | std::ios::trunc);
        out << bytes;
        if (!out) throw app::Failure("cannot write " + config::to_utf8(p));
        made.files.push_back(p);
    }
    if (!m.next.empty()) made.next = render(env, m.next, data, "next");
    return made;
}

}
