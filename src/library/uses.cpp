#include "library/uses.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <iterator>
#include <regex>

#include "app/failure.hpp"
#include "config/config.hpp"

namespace library {

static const char* rant_git = "https://github.com/KosmosisDire/Rant.git";
static const char* python_package = "rant-middleware";
static const char* dist_info_prefix = "rant_middleware-";
static const char* dotnet_package = "Rant";

const char* kind_name(Kind k) {
    switch (k) {
    case Kind::CMake:  return "cmake";
    case Kind::Python: return "python";
    case Kind::CSharp: return "csharp";
    }
    return "?";
}

static std::string lower(std::string s) {
    for (auto& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}

static bool iequals(const std::string& a, const std::string& b) { return lower(a) == lower(b); }

static std::regex icase(const char* pattern) { return std::regex(pattern, std::regex::ECMAScript | std::regex::icase); }

/* Every match of re in text with its third group, the version, made version. Built by hand,
 * since a format string would read "$2" followed by a digit as a group of two digits. */
static std::string with_version(const std::string& text, const std::regex& re, const std::string& version) {
    std::string out;
    size_t last = 0;
    for (auto it = std::sregex_iterator(text.begin(), text.end(), re); it != std::sregex_iterator(); ++it) {
        const std::smatch& m = *it;
        out += text.substr(last, m.position(3) - last) + version;
        last = m.position(3) + m.length(3);
    }
    return out + text.substr(last);
}

static Use use_of(Kind kind, const std::string& how, const fs::path& dir, const fs::path& file) {
    Use u;
    u.kind = kind;
    u.how = how;
    u.dir = dir;
    u.file = file;
    return u;
}

// ---- CMake ----

/* One command call: its name, the text between its parentheses, and where it ends. */
struct Call {
    std::string name;
    size_t      args_begin = 0, args_end = 0;
    size_t      end = 0;    /* just past the closing parenthesis */

    std::string args(const std::string& text) const { return text.substr(args_begin, args_end - args_begin); }
};

/* The length of a bracket opening such as `[[` or `[==[` at i, and its count of `=`. */
static bool bracket_open(const std::string& t, size_t i, size_t* len, size_t* eqs) {
    if (i >= t.size() || t[i] != '[') return false;
    size_t j = i + 1;
    while (j < t.size() && t[j] == '=') j++;
    if (j >= t.size() || t[j] != '[') return false;
    *len = j + 1 - i;
    *eqs = j - i - 1;
    return true;
}

/* The offset past a bracket argument or comment opened at i. */
static size_t skip_bracket(const std::string& t, size_t i, size_t len, size_t eqs) {
    std::string close = "]" + std::string(eqs, '=') + "]";
    size_t k = t.find(close, i + len);
    return k == std::string::npos ? t.size() : k + close.size();
}

/* Every command call, past comments, quoted and bracket arguments. */
static std::vector<Call> cmake_calls(const std::string& t) {
    std::vector<Call> calls;
    size_t i = 0, len = 0, eqs = 0;
    while (i < t.size()) {
        char c = t[i];
        if (c == '#') {
            if (bracket_open(t, i + 1, &len, &eqs)) i = skip_bracket(t, i + 1, len, eqs);
            else i = std::min(t.find('\n', i), t.size());
            continue;
        }
        if (!(std::isalpha((unsigned char)c) || c == '_')) {
            i++;
            continue;
        }
        size_t start = i;
        while (i < t.size() && (std::isalnum((unsigned char)t[i]) || t[i] == '_')) i++;
        std::string name = t.substr(start, i - start);
        size_t j = i;
        while (j < t.size() && (t[j] == ' ' || t[j] == '\t')) j++;
        if (j >= t.size() || t[j] != '(') continue;
        size_t open = j + 1, k = open;
        int depth = 1;
        while (k < t.size() && depth > 0) {
            char d = t[k];
            if (d == '(') depth++;
            else if (d == ')') depth--;
            else if (d == '"') {
                for (k++; k < t.size() && t[k] != '"'; k += t[k] == '\\' ? 2 : 1) {}
            } else if (d == '#') {
                if (bracket_open(t, k + 1, &len, &eqs)) k = skip_bracket(t, k + 1, len, eqs) - 1;
                else k = std::min(t.find('\n', k), t.size()) - 1;
            } else if (d == '[' && bracket_open(t, k, &len, &eqs)) {
                k = skip_bracket(t, k, len, eqs) - 1;
            }
            k++;
        }
        size_t end = std::min(k, t.size());
        calls.push_back({ name, open, std::max(open, end ? end - 1 : 0), end });
        i = end;
    }
    return calls;
}

static bool named(const Call& c, std::initializer_list<const char*> names) {
    for (auto* n : names)
        if (iequals(c.name, n)) return true;
    return false;
}

static std::string first_arg(const std::string& args) {
    size_t b = args.find_first_not_of(" \t\r\n");
    if (b == std::string::npos) return "";
    size_t e = args.find_first_of(" \t\r\n)", b);
    std::string a = args.substr(b, e == std::string::npos ? std::string::npos : e - b);
    if (a.size() >= 2 && a.front() == '"' && a.back() == '"') a = a.substr(1, a.size() - 2);
    return a;
}

/* A fetch of Rant: named rant, or pointing at the Rant repository. */
static bool fetches_rant(const std::string& text, const Call& c) {
    if (!named(c, { "CPMAddPackage", "CPMFindPackage", "CPMDeclarePackage", "FetchContent_Declare" })) return false;
    std::string args = c.args(text);
    static const std::regex name = icase(R"(\bNAME\s+rant\b)");
    return lower(args).find("kosmosisdire/rant") != std::string::npos || iequals(first_arg(args), "rant") ||
           std::regex_search(args, name);
}

/* Where a version sits in a fetch: after GIT_TAG or VERSION, after @ in the CPM short form,
 * in a release download or archive URL. */
static const std::regex& version_pattern() {
    static const std::regex v = icase(R"((GIT_TAG\s+"?|VERSION\s+"?|@|/download/|/archive/(?:refs/tags/)?)(v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z]+)?))");
    return v;
}

std::optional<Use> cmake_use(const std::string& text) {
    for (auto& c : cmake_calls(text)) {
        std::string args = c.args(text);
        Use u = use_of(Kind::CMake, "", {}, {});
        if (fetches_rant(text, c)) {
            u.how = lower(c.name).rfind("cpm", 0) == 0 ? "CPM" : "FetchContent";
            std::smatch m;
            if (std::regex_search(args, m, version_pattern())) u.version = m[3].str();
        } else if (named(c, { "find_package" }) && iequals(first_arg(args), "rant")) {
            u.how = "find_package";
            static const std::regex v(R"(^\s*\S+\s+(\d[^\s)]*))");
            std::smatch m;
            if (std::regex_search(args, m, v)) u.version = m[1].str();
        } else {
            continue;
        }
        return u;
    }
    return std::nullopt;
}

std::string cmake_set(const std::string& text, const std::string& version) {
    for (auto& c : cmake_calls(text)) {
        if (!fetches_rant(text, c)) continue;
        std::string args = c.args(text);
        if (!std::regex_search(args, version_pattern()))
            throw app::Failure("its Rant fetch names no version to move, set GIT_TAG v" + version + " by hand");
        std::string moved = with_version(args, version_pattern(), version);
        return text.substr(0, c.args_begin) + moved + text.substr(c.args_end);
    }
    throw app::Failure("it uses an installed Rant through find_package, update that install instead");
}

std::string cmake_add(const std::string& text, const std::string& version, std::string* hint) {
    std::vector<Call> calls = cmake_calls(text);
    auto cpm_setup = [&](const Call& c) {
        return (named(c, { "include" }) && lower(c.args(text)).find("cpm") != std::string::npos) || named(c, { "CPMAddPackage" });
    };
    bool cpm = std::any_of(calls.begin(), calls.end(), cpm_setup);
    std::optional<size_t> anchor;
    for (auto& c : calls)
        if (named(c, { "project", "FetchContent_MakeAvailable" }) || cpm_setup(c)) anchor = std::max(anchor.value_or(0), c.end);

    std::string block;
    if (cpm) {
        block = "CPMAddPackage(NAME rant\n              GIT_REPOSITORY " + std::string(rant_git) + "\n              GIT_TAG v" + version + ")\n";
    } else {
        bool included = std::any_of(calls.begin(), calls.end(), [&](const Call& c) { return named(c, { "include" }) && first_arg(c.args(text)) == "FetchContent"; });
        block = std::string(included ? "" : "include(FetchContent)\n") + "FetchContent_Declare(rant\n  GIT_REPOSITORY " + rant_git +
                "\n  GIT_TAG v" + version + "\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable(rant)\n";
    }

    std::vector<const Call*> targets;
    for (auto& c : calls) {
        if (!named(c, { "add_executable", "add_library" })) continue;
        std::string args = c.args(text);
        std::transform(args.begin(), args.end(), args.begin(), [](unsigned char ch) { return (char)std::toupper(ch); });
        if (args.find(" IMPORTED") == std::string::npos && args.find(" ALIAS") == std::string::npos && args.find(" INTERFACE") == std::string::npos)
            targets.push_back(&c);
    }
    bool linked = std::any_of(calls.begin(), calls.end(), [&](const Call& c) { return named(c, { "target_link_libraries" }) && c.args(text).find("rant::") != std::string::npos; });
    std::optional<std::pair<size_t, std::string>> link;
    if (targets.size() == 1 && !linked) {
        std::string name = first_arg(targets[0]->args(text));
        /* a target linked once without PRIVATE, PUBLIC or INTERFACE must stay without */
        bool plain = false;
        for (auto& c : calls) {
            if (!named(c, { "target_link_libraries" }) || first_arg(c.args(text)) != name) continue;
            static const std::regex keyword(R"(\b(PRIVATE|PUBLIC|INTERFACE)\b)");
            plain = !std::regex_search(c.args(text), keyword);
        }
        link = { { targets[0]->end, "target_link_libraries(" + name + (plain ? " " : " PRIVATE ") + "rant::rant_host)\n" } };
    }

    auto line_end = [&](size_t at) {
        size_t k = text.find('\n', at);
        return k == std::string::npos ? text.size() : k + 1;
    };
    std::vector<std::pair<size_t, std::string>> edits;
    edits.push_back(anchor ? std::make_pair(line_end(*anchor), "\n" + block) : std::make_pair(size_t(0), block + "\n"));
    if (link) edits.push_back({ line_end(link->first), link->second });
    std::sort(edits.begin(), edits.end(), [](auto& a, auto& b) { return a.first > b.first; });
    std::string out = text;
    for (auto& [at, insert] : edits) {
        std::string piece = at == out.size() && !out.empty() && out.back() != '\n' ? "\n" + insert : insert;
        out.insert(at, piece);
    }
    if (hint) *hint = link || linked ? "" : "target_link_libraries(<your target> PRIVATE rant::rant_host)";
    return out;
}

std::string cmake_add_node(const std::string& text, const std::string& name, const std::string& source) {
    std::optional<size_t> after;
    for (auto& c : cmake_calls(text)) {
        if (!named(c, { "add_executable", "add_library", "target_link_libraries" })) continue;
        if (!named(c, { "target_link_libraries" }) && first_arg(c.args(text)) == name)
            throw app::Failure("CMakeLists.txt has a target `" + name + "` already, nothing was written");
        after = c.end;
    }
    std::string lines = "add_executable(" + name + " " + source + ")\ntarget_link_libraries(" + name + " PRIVATE rant::rant_host)\n";
    std::string out = text;
    if (!out.empty() && out.back() != '\n') out += '\n';
    if (!after) return out + (out.empty() ? "" : "\n") + lines;
    size_t at = out.find('\n', *after);
    return out.insert(at == std::string::npos ? out.size() : at + 1, lines);
}

// ---- Python ----

static const std::regex& pin_pattern() {
    static const std::regex p = icase(R"((["']\s*rant[-_.]middleware\s*(?:\[[^\]]*\])?\s*(?:===|==|~=|>=|<=|!=|>|<)\s*)(v?)(\d+(?:\.\d+)*(?:[-.]?[0-9A-Za-z]+)*))");
    return p;
}

std::string pyproject_set(const std::string& text, const std::string& version) {
    if (!std::regex_search(text, pin_pattern())) throw app::Failure(std::string("its ") + python_package + " dependency names no version");
    return with_version(text, pin_pattern(), version);
}

/* The version installed in a venv, from its dist-info folder. */
static std::optional<std::string> venv_version(const fs::path& venv) {
    std::vector<fs::path> sites = { venv / "Lib" / "site-packages" };
    std::error_code ec;
    for (auto& e : fs::directory_iterator(venv / "lib", ec)) sites.push_back(e.path() / "site-packages");
    std::string prefix = dist_info_prefix;
    for (auto& site : sites)
        for (auto& e : fs::directory_iterator(site, ec)) {
            std::string name = e.path().filename().u8string();
            const std::string tail = ".dist-info";
            if (name.rfind(prefix, 0) == 0 && name.size() > prefix.size() + tail.size() && name.compare(name.size() - tail.size(), tail.size(), tail) == 0)
                return name.substr(prefix.size(), name.size() - prefix.size() - tail.size());
        }
    return std::nullopt;
}

static std::string read_file(const fs::path& p) {
    std::ifstream in(p, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

/* A venv's own python. */
static fs::path python_in(const fs::path& venv) {
#ifdef _WIN32
    return venv / "Scripts" / "python.exe";
#else
    return venv / "bin" / "python";
#endif
}

static bool has_python(const fs::path& dir) {
    std::error_code ec;
    for (auto& e : fs::directory_iterator(dir, ec))
        if (e.path().extension() == ".py") return true;
    return false;
}

/* The pyproject pin and the venv of a folder of Python. A folder named explicitly counts for
 * any Python file, else only for Python that imports rant. */
static void python_uses(const fs::path& dir, bool explicit_folder, std::vector<Use>& out) {
    fs::path pyproject = dir / "pyproject.toml";
    std::string text = read_file(pyproject);
    static const std::regex dependency = icase(R"(["']\s*rant[-_.]middleware\b)");
    bool listed = std::regex_search(text, dependency);
    if (listed) {
        Use u = use_of(Kind::Python, "pyproject", dir, pyproject);
        std::smatch m;
        if (std::regex_search(text, m, pin_pattern())) u.version = m[3].str();
        out.push_back(u);
    }
    config::PythonInfo py = config::python_of(dir);
    bool counts = listed || py.imports_rant || (explicit_folder && (fs::exists(pyproject) || has_python(dir)));
    if (!counts) return;
    Use u = use_of(Kind::Python, "venv", dir, {});
    if (py.venv) {
        u.file = *py.venv;
        u.venv_exists = true;
        u.venv_python = py.venv_python;
        u.version = venv_version(*py.venv);
    } else {
        /* a new venv goes beside the pyproject, else at the workspace root, else here */
        u.file = (fs::exists(pyproject) || !py.root ? dir : *py.root) / ".venv";
        u.venv_python = python_in(u.file);
        u.python = py.interpreter;
    }
    out.push_back(u);
}

// ---- C# ----

static const std::regex& reference_pattern() {
    static const std::regex r = icase(R"(<PackageReference\b[^>]*?\bInclude\s*=\s*"Rant"[^>]*?(?:/>|>[\s\S]*?</PackageReference\s*>))");
    return r;
}

/* A NuGet version as an attribute or a child element, whole: 0.0.16, [0.0.16] or a range. */
static const std::regex& element_version() {
    static const std::regex v = icase(R"((\bVersion\s*=\s*"|<Version>\s*)([^"<]*?)(\s*(?:"|</Version>)))");
    return v;
}

/* The lowest version a NuGet version or range allows: [0.0.16,) is 0.0.16. */
static std::optional<std::string> nuget_version(std::string v) {
    v.erase(0, v.find_first_not_of("[( \t"));
    v = v.substr(0, v.find(','));
    while (!v.empty() && (v.back() == ']' || v.back() == ')' || std::isspace((unsigned char)v.back()))) v.pop_back();
    return v.empty() ? std::nullopt : std::optional<std::string>(v);
}

/* An exact pin. A plain version means that version or later, and nuget.org has an unrelated
 * package named Rant at 1.0, so a missing feed would quietly build against it. */
static std::string exact(const std::string& version) { return "[" + version + "]"; }

std::optional<Use> csharp_use(const std::string& text) {
    std::smatch m;
    if (std::regex_search(text, m, reference_pattern())) {
        Use u = use_of(Kind::CSharp, "PackageReference", {}, {});
        std::string element = m.str();
        std::smatch v;
        if (std::regex_search(element, v, element_version())) u.version = nuget_version(v[2].str());
        return u;
    }
    static const std::regex project = icase(R"(<ProjectReference\s+Include\s*=\s*"[^"]*[/\\]Rant\.csproj")");
    if (std::regex_search(text, project)) return use_of(Kind::CSharp, "ProjectReference", {}, {});
    return std::nullopt;
}

std::string csharp_set(const std::string& text, const std::string& version) {
    std::smatch m;
    if (!std::regex_search(text, m, reference_pattern()))
        throw app::Failure("it references the Rant project from source, update that checkout instead");
    std::string element = m.str(), moved;
    std::smatch v;
    if (std::regex_search(element, v, element_version()))
        moved = v.prefix().str() + v[1].str() + exact(version) + v[3].str() + v.suffix().str();
    else
        moved = std::regex_replace(element, icase(R"("Rant")"), "\"Rant\" Version=\"" + exact(version) + "\"", std::regex_constants::format_first_only);
    return m.prefix().str() + moved + m.suffix().str();
}

std::string csharp_add(const std::string& text, const std::string& version) {
    std::string reference = std::string("<PackageReference Include=\"") + dotnet_package + "\" Version=\"" + exact(version) + "\" />";
    static const std::regex last = icase(R"(\n([ \t]*)<PackageReference\b[^>]*?(?:/>|>[\s\S]*?</PackageReference\s*>))");
    std::smatch found;
    for (auto it = std::sregex_iterator(text.begin(), text.end(), last); it != std::sregex_iterator(); ++it) found = *it;
    if (!found.empty()) {
        size_t at = found.position(0) + found.length(0);
        return text.substr(0, at) + "\n" + found[1].str() + reference + text.substr(at);
    }
    size_t end = text.rfind("</Project>");
    if (end == std::string::npos) throw app::Failure("it has no </Project> to add to");
    return text.substr(0, end) + "  <ItemGroup>\n    " + reference + "\n  </ItemGroup>\n" + text.substr(end);
}

// ---- finding and editing ----

static std::vector<Use> folder_uses(const fs::path& dir, bool explicit_folder) {
    std::vector<Use> out;
    fs::path cmake = dir / "CMakeLists.txt";
    if (fs::is_regular_file(cmake))
        if (auto u = cmake_use(read_file(cmake))) {
            u->dir = dir;
            u->file = cmake;
            out.push_back(*u);
        }
    std::vector<fs::path> projects;
    std::error_code ec;
    for (auto& e : fs::directory_iterator(dir, ec))
        if (lower(e.path().extension().u8string()) == ".csproj") projects.push_back(e.path());
    std::sort(projects.begin(), projects.end());
    for (auto& p : projects)
        if (auto u = csharp_use(read_file(p))) {
            u->dir = dir;
            u->file = p;
            out.push_back(*u);
        }
    python_uses(dir, explicit_folder, out);
    return out;
}

std::vector<Use> in_folder(const fs::path& dir) { return folder_uses(dir, true); }

std::vector<Use> under(const fs::path& start) {
    config::Folders walk = config::folders_under(start);
    std::vector<Use> out;
    auto listed = [&](const fs::path& venv) {
        return std::any_of(out.begin(), out.end(), [&](const Use& o) { return o.how == "venv" && o.file == venv; });
    };
    for (auto& d : walk.folders)
        for (auto& u : folder_uses(d, false)) {
            if (u.how == "venv" && u.venv_exists) {
                if (listed(u.file)) continue;
                u.dir = u.file.parent_path();
            }
            out.push_back(u);
        }
    for (auto& venv : walk.venvs) {
        if (listed(venv)) continue;
        Use u = use_of(Kind::Python, "venv", venv.parent_path(), venv);
        u.venv_exists = true;
        u.version = venv_version(venv);
        u.wanted = u.version.has_value();
        u.venv_python = python_in(venv);
        out.push_back(u);
    }
    std::stable_sort(out.begin(), out.end(), [](const Use& a, const Use& b) { return a.dir < b.dir; });
    return out;
}

static void edit(const fs::path& file, const std::string& changed, const std::string& text) {
    if (changed == text) return;
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    out << changed;
    if (!out) throw app::Failure("cannot write " + file.u8string());
}

void set_version(const fs::path& file, const std::string& version) {
    std::string text = read_file(file);
    std::string name = lower(file.filename().u8string());
    if (name == "cmakelists.txt") edit(file, cmake_set(text, version), text);
    else if (name == "pyproject.toml") edit(file, pyproject_set(text, version), text);
    else if (lower(file.extension().u8string()) == ".csproj") edit(file, csharp_set(text, version), text);
    else throw app::Failure(file.u8string() + " names no Rant version rant can move");
}

std::string add(Kind kind, const fs::path& file, const std::string& version) {
    std::string text = read_file(file), hint;
    if (kind == Kind::CMake) edit(file, cmake_add(text, version, &hint), text);
    else if (kind == Kind::CSharp) edit(file, csharp_add(text, version), text);
    else throw app::Failure("Python takes Rant into its venv, not its files");
    return hint;
}

void add_node(const fs::path& cmakelists, const std::string& name, const std::string& source) {
    std::string text = read_file(cmakelists);
    edit(cmakelists, cmake_add_node(text, name, source), text);
}

}
