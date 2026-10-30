#include "library/uses.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <iterator>
#include <regex>

#include "app/failure.hpp"
#include "config/config.hpp"

namespace library {

const char* kind_name(Kind k) {
    switch (k) {
    case Kind::CMake:  return "cmake";
    case Kind::Python: return "python";
    case Kind::CSharp: return "csharp";
    case Kind::Rust:   return "rust";
    }
    return "?";
}

bool supports(const Library& lib, Kind k) {
    switch (k) {
    case Kind::CMake:  return !lib.names.cmake.empty();
    case Kind::Python: return !lib.names.python.empty();
    case Kind::CSharp: return !lib.names.csharp.empty();
    case Kind::Rust:   return !lib.names.rust.empty();
    }
    return false;
}

static std::string lower(std::string s) {
    for (auto& c : s) c = (char)std::tolower((unsigned char)c);
    return s;
}

static bool iequals(const std::string& a, const std::string& b) { return lower(a) == lower(b); }

static std::regex icase(const std::string& pattern) { return std::regex(pattern, std::regex::ECMAScript | std::regex::icase); }

/* s in a pattern, matched as written. */
static std::string escaped(const std::string& s) {
    std::string out;
    for (char c : s) {
        if (std::string(R"(\^$.|?*+()[]{})").find(c) != std::string::npos) out += '\\';
        out += c;
    }
    return out;
}

static std::string git_url(const Library& lib) { return "https://github.com/" + lib.repo + ".git"; }

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

static Use use_of(const Library& lib, Kind kind, const std::string& how, const fs::path& dir, const fs::path& file) {
    Use u;
    u.library = lib.name;
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

/* A fetch of the library: named after it, or pointing at its repo. */
static bool fetches(const std::string& text, const Call& c, const Library& lib) {
    if (!named(c, { "CPMAddPackage", "CPMFindPackage", "CPMDeclarePackage", "FetchContent_Declare" })) return false;
    std::string args = c.args(text);
    std::regex name = icase(R"(\bNAME\s+)" + escaped(lib.name) + R"((?![\w-]))");
    std::regex repo = icase(R"((^|[/:"\s]))" + escaped(lib.repo) + R"((\.git)?(?![\w.-]))");
    return std::regex_search(args, repo) || iequals(first_arg(args), lib.name) || std::regex_search(args, name);
}

/* Whether a link names the library's target, or another of its namespace such as rant::rant. */
static bool links(const std::string& args, const Library& lib) {
    const std::string& t = lib.names.cmake;
    size_t ns = t.find("::");
    std::string target = ns == std::string::npos ? escaped(t) + R"((?![\w:]))" : escaped(t.substr(0, ns + 2));
    return std::regex_search(args, std::regex(R"((^|[^\w:]))" + target));
}

/* Where a version sits in a fetch: after GIT_TAG or VERSION, after @ in the CPM short form,
 * in a release download or archive URL. */
static const std::regex& version_pattern() {
    static const std::regex v = icase(R"((GIT_TAG\s+"?|VERSION\s+"?|@|/download/|/archive/(?:refs/tags/)?)(v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z]+)?))");
    return v;
}

std::optional<Use> cmake_use(const std::string& text, const Library& lib) {
    for (auto& c : cmake_calls(text)) {
        std::string args = c.args(text);
        Use u = use_of(lib, Kind::CMake, "", {}, {});
        if (fetches(text, c, lib)) {
            u.how = lower(c.name).rfind("cpm", 0) == 0 ? "CPM" : "FetchContent";
            std::smatch m;
            if (std::regex_search(args, m, version_pattern())) u.version = m[3].str();
        } else if (named(c, { "find_package" }) && iequals(first_arg(args), lib.name)) {
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

std::string cmake_set(const std::string& text, const Library& lib, const std::string& version) {
    for (auto& c : cmake_calls(text)) {
        if (!fetches(text, c, lib)) continue;
        std::string args = c.args(text);
        if (!std::regex_search(args, version_pattern()))
            throw app::Failure("its " + lib.name + " fetch names no version to move, set GIT_TAG v" + version + " by hand");
        std::string moved = with_version(args, version_pattern(), version);
        return text.substr(0, c.args_begin) + moved + text.substr(c.args_end);
    }
    throw app::Failure("it uses an installed " + lib.name + " through find_package, update that install instead");
}

std::string cmake_add(const std::string& text, const Library& lib, const std::string& tag, std::string* hint) {
    std::vector<Call> calls = cmake_calls(text);
    auto cpm_setup = [&](const Call& c) {
        return (named(c, { "include" }) && lower(c.args(text)).find("cpm") != std::string::npos) || named(c, { "CPMAddPackage" });
    };
    bool cpm = std::any_of(calls.begin(), calls.end(), cpm_setup);
    std::optional<size_t> anchor;
    for (auto& c : calls)
        if (named(c, { "project", "FetchContent_MakeAvailable" }) || cpm_setup(c)) anchor = std::max(anchor.value_or(0), c.end);

    std::string block, url = git_url(lib);
    if (cpm) {
        block = "CPMAddPackage(NAME " + lib.name + "\n              GIT_REPOSITORY " + url + "\n              GIT_TAG " + tag + ")\n";
    } else {
        bool included = std::any_of(calls.begin(), calls.end(), [&](const Call& c) { return named(c, { "include" }) && first_arg(c.args(text)) == "FetchContent"; });
        block = std::string(included ? "" : "include(FetchContent)\n") + "FetchContent_Declare(" + lib.name + "\n  GIT_REPOSITORY " + url +
                "\n  GIT_TAG " + tag + "\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable(" + lib.name + ")\n";
    }

    std::vector<const Call*> targets;
    for (auto& c : calls) {
        if (!named(c, { "add_executable", "add_library" })) continue;
        std::string args = c.args(text);
        std::transform(args.begin(), args.end(), args.begin(), [](unsigned char ch) { return (char)std::toupper(ch); });
        if (args.find(" IMPORTED") == std::string::npos && args.find(" ALIAS") == std::string::npos && args.find(" INTERFACE") == std::string::npos)
            targets.push_back(&c);
    }
    bool linked = std::any_of(calls.begin(), calls.end(), [&](const Call& c) { return named(c, { "target_link_libraries" }) && links(c.args(text), lib); });
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
        link = { { targets[0]->end, "target_link_libraries(" + name + (plain ? " " : " PRIVATE ") + lib.names.cmake + ")\n" } };
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
    if (hint) *hint = link || linked ? "" : "target_link_libraries(<your target> PRIVATE " + lib.names.cmake + ")";
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

static std::string read_file(const fs::path& p);

/* The words of a call's arguments: quoted ones whole and unquoted, bracket arguments left out. */
static std::vector<std::string> cmake_words(const std::string& args) {
    std::vector<std::string> out;
    size_t i = 0, len = 0, eqs = 0;
    while (i < args.size()) {
        char c = args[i];
        if (std::isspace((unsigned char)c)) {
            i++;
            continue;
        }
        if (c == '"') {
            size_t k = i + 1;
            while (k < args.size() && args[k] != '"') k += args[k] == '\\' ? 2 : 1;
            out.push_back(args.substr(i + 1, std::min(k, args.size()) - i - 1));
            i = k + 1;
            continue;
        }
        if (c == '[' && bracket_open(args, i, &len, &eqs)) {
            i = skip_bracket(args, i, len, eqs);
            continue;
        }
        size_t k = i;
        while (k < args.size() && !std::isspace((unsigned char)args[k])) k++;
        out.push_back(args.substr(i, k - i));
        i = k;
    }
    return out;
}

std::optional<std::string> cmake_source(const std::string& text, const std::string& target) {
    static const std::regex source(R"(\.(c|cc|cpp|cxx|c\+\+|cu|m|mm)$)", std::regex::icase);
    for (auto& c : cmake_calls(text)) {
        if (!named(c, { "add_executable", "target_sources" })) continue;
        std::vector<std::string> words = cmake_words(c.args(text));
        if (words.empty() || words[0] != target) continue;
        for (size_t i = 1; i < words.size(); i++) {
            const std::string& w = words[i];
            if (w.find("${") != std::string::npos || w.find("$<") != std::string::npos) continue;
            if (std::regex_search(w, source)) return w;
        }
    }
    return std::nullopt;
}

bool csharp_main(const std::string& text) {
    static const std::regex main(R"(\bstatic\s+(async\s+)?(void|int|Task|Task\s*<\s*int\s*>)\s+Main\s*\()");
    return std::regex_search(text, main);
}

/* The source of a CMake node: its executable's name looked up in every CMakeLists of the package. */
static std::optional<fs::path> cmake_node_source(const config::NodeType& n) {
    std::vector<std::string> targets = { n.name };
    if (n.path && n.path->stem().u8string() != n.name) targets.insert(targets.begin(), n.path->stem().u8string());
    for (auto& dir : config::folders_under(n.cwd).folders) {
        fs::path cmake = dir / "CMakeLists.txt";
        if (!fs::is_regular_file(cmake)) continue;
        std::string text = read_file(cmake);
        for (auto& t : targets)
            if (auto s = cmake_source(text, t)) {
                fs::path file = dir / config::from_utf8(*s);
                if (fs::is_regular_file(file)) return file;
            }
    }
    return std::nullopt;
}

/* The source of a C# node: the file with Main in its project, else Program.cs. */
static std::optional<fs::path> csharp_node_source(const config::NodeType& n) {
    fs::path project = n.cwd;
    for (auto& dir : config::folders_under(n.cwd).folders) {
        std::error_code ec;
        if (fs::is_regular_file(dir / (n.name + ".csproj"), ec)) {
            project = dir;
            break;
        }
    }
    for (auto& dir : config::folders_under(project).folders) {
        fs::path rel = dir.lexically_relative(project);
        if (std::any_of(rel.begin(), rel.end(), [](const fs::path& p) { return lower(p.u8string()) == "bin" || lower(p.u8string()) == "obj"; }))
            continue;
        std::error_code ec;
        for (auto& e : fs::directory_iterator(dir, ec))
            if (lower(e.path().extension().u8string()) == ".cs" && csharp_main(read_file(e.path()))) return e.path();
    }
    if (fs::is_regular_file(project / "Program.cs")) return project / "Program.cs";
    return std::nullopt;
}

std::optional<fs::path> node_source(const config::NodeType& n) {
    switch (n.kind) {
    case config::NodeKind::Python:
        return n.path;
    case config::NodeKind::Native:
        return cmake_node_source(n);
    case config::NodeKind::CSharp:
        return csharp_node_source(n);
    case config::NodeKind::Rust:
        return n.source;
    default:
        return std::nullopt;
    }
}

// ---- Python ----

/* A Python project's name in a pattern, as pip compares names: any case, and -, _ and . alike. */
static std::string python_name(const std::string& project) {
    std::string out;
    for (char c : project) out += c == '-' || c == '_' || c == '.' ? std::string("[-_.]+") : escaped(std::string(1, c));
    return out;
}

static std::regex pin_pattern(const Library& lib) {
    return icase(R"((["']\s*)" + python_name(lib.names.python) +
                 R"(\s*(?:\[[^\]]*\])?\s*(?:===|==|~=|>=|<=|!=|>|<)\s*)(v?)(\d+(?:\.\d+)*(?:[-.]?[0-9A-Za-z]+)*))");
}

std::string pyproject_set(const std::string& text, const Library& lib, const std::string& version) {
    if (!std::regex_search(text, pin_pattern(lib))) throw app::Failure("its " + lib.names.python + " dependency names no version");
    return with_version(text, pin_pattern(lib), version);
}

std::string dist_name(const Library& lib) {
    std::string out;
    for (char c : lower(lib.names.python))
        if (c != '-' && c != '_' && c != '.') out += c;
        else if (out.empty() || out.back() != '_') out += '_';
    return out;
}

/* The version of the library installed in a venv, from the dist-info folder its wheel left. */
static std::optional<std::string> venv_version(const fs::path& venv, const Library& lib) {
    std::vector<fs::path> sites = { venv / "Lib" / "site-packages" };
    std::error_code ec;
    for (auto& e : fs::directory_iterator(venv / "lib", ec)) sites.push_back(e.path() / "site-packages");
    std::string prefix = dist_name(lib) + "-";
    for (auto& site : sites)
        for (auto& e : fs::directory_iterator(site, ec)) {
            std::string name = lower(e.path().filename().u8string());
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

/* The pyproject pin and venv of a folder for each library it lists or holds. Python that imports
 * rant is a node, so it counts for the default library, and adding counts any Python. */
static void python_uses(const fs::path& dir, const std::vector<const Library*>& libs, bool adding, std::vector<Use>& out) {
    if (libs.empty()) return;
    fs::path pyproject = dir / "pyproject.toml";
    std::string text = read_file(pyproject);
    config::PythonInfo py = config::python_of(dir);
    for (const Library* lib : libs) {
        std::regex dependency = icase(R"(["']\s*)" + python_name(lib->names.python) + R"((?![\w.-]))");
        bool listed = std::regex_search(text, dependency);
        if (listed) {
            Use u = use_of(*lib, Kind::Python, "pyproject", dir, pyproject);
            std::smatch m;
            if (std::regex_search(text, m, pin_pattern(*lib))) u.version = m[3].str();
            out.push_back(u);
        }
        std::optional<std::string> held = py.venv ? venv_version(*py.venv, *lib) : std::nullopt;
        bool node = lib->name == default_library && py.imports_rant;
        if (!(listed || held || node || (adding && (fs::exists(pyproject) || has_python(dir))))) continue;
        Use u = use_of(*lib, Kind::Python, "venv", dir, {});
        if (py.venv) {
            u.file = *py.venv;
            u.venv_exists = true;
            u.venv_python = py.venv_python;
            u.version = held;
        } else {
            /* a new venv goes beside the pyproject, else at the workspace root, else here */
            u.file = (fs::exists(pyproject) || !py.root ? dir : *py.root) / ".venv";
            u.venv_python = python_in(u.file);
            u.python = py.interpreter;
        }
        out.push_back(u);
    }
}

// ---- C# ----

static std::regex reference_pattern(const Library& lib) {
    return icase(R"(<PackageReference\b[^>]*?\bInclude\s*=\s*")" + escaped(lib.names.csharp) + R"("[^>]*?(?:/>|>[\s\S]*?</PackageReference\s*>))");
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

/* An exact pin. A plain version means that or later, and nuget.org may hold an unrelated package
 * of the id, as it does Rant 1.0, so a missing feed would quietly build against it. */
static std::string exact(const std::string& version) { return "[" + version + "]"; }

std::optional<Use> csharp_use(const std::string& text, const Library& lib) {
    std::smatch m;
    if (std::regex_search(text, m, reference_pattern(lib))) {
        Use u = use_of(lib, Kind::CSharp, "PackageReference", {}, {});
        std::string element = m.str();
        std::smatch v;
        if (std::regex_search(element, v, element_version())) u.version = nuget_version(v[2].str());
        return u;
    }
    std::regex project = icase(R"(<ProjectReference\s+Include\s*=\s*"[^"]*[/\\])" + escaped(lib.names.csharp) + R"(\.csproj")");
    if (std::regex_search(text, project)) return use_of(lib, Kind::CSharp, "ProjectReference", {}, {});
    return std::nullopt;
}

std::string csharp_set(const std::string& text, const Library& lib, const std::string& version) {
    std::smatch m;
    if (!std::regex_search(text, m, reference_pattern(lib)))
        throw app::Failure("it references the " + lib.names.csharp + " project from source, update that checkout instead");
    std::string element = m.str(), moved;
    std::smatch v;
    if (std::regex_search(element, v, element_version()))
        moved = v.prefix().str() + v[1].str() + exact(version) + v[3].str() + v.suffix().str();
    else
        moved = std::regex_replace(element, icase("\"" + escaped(lib.names.csharp) + "\""), "\"" + lib.names.csharp + "\" Version=\"" + exact(version) + "\"",
                                   std::regex_constants::format_first_only);
    return m.prefix().str() + moved + m.suffix().str();
}

std::string csharp_add(const std::string& text, const Library& lib, const std::string& version) {
    std::string reference = "<PackageReference Include=\"" + lib.names.csharp + "\" Version=\"" + exact(version) + "\" />";
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

// ---- Rust ----

static bool ends_with(const std::string& s, const std::string& tail) {
    return s.size() >= tail.size() && s.compare(s.size() - tail.size(), tail.size(), tail) == 0;
}

/* [dependencies], [workspace.dependencies] or [target.'cfg(unix)'.dependencies] */
static bool dependency_table(const std::string& section) { return section == "dependencies" || ends_with(section, ".dependencies"); }

/* The library's dependency in a Cargo.toml as the text it spans: a `<crate> = ...` line, or a
 * [dependencies.<crate>] table, also renamed with `package`. */
static std::optional<std::pair<size_t, size_t>> cargo_entry(const std::string& text, const Library& lib) {
    const std::string& crate = lib.names.rust;
    std::regex renamed = icase(R"(\bpackage\s*=\s*")" + escaped(crate) + "\"");
    static const std::regex key = icase(R"re(^\s*"?([A-Za-z0-9_-]+)"?\s*=)re");
    std::string section;
    std::optional<size_t> table;    /* the start of an open [*.dependencies.<name>] table */
    std::string table_name;
    size_t at = 0;
    while (at <= text.size()) {
        size_t end = std::min(text.find('\n', at), text.size());
        std::string line = text.substr(at, end - at);
        size_t first = line.find_first_not_of(" \t");
        bool header = first != std::string::npos && line[first] == '[';
        if (header && table) {
            std::string body = text.substr(*table, at - *table);
            if (table_name == crate || std::regex_search(body, renamed)) return std::make_pair(*table, at);
            table.reset();
        }
        if (header) {
            size_t close = line.find(']', first);
            section = line.substr(first + 1, close == std::string::npos ? std::string::npos : close - first - 1);
            size_t dot = section.rfind('.');
            if (dot != std::string::npos && dependency_table(section.substr(0, dot))) {
                table = at;
                table_name = section.substr(dot + 1);
            }
        } else if (!table && dependency_table(section)) {
            std::smatch m;
            if (std::regex_search(line, m, key) && (m[1].str() == crate || std::regex_search(line, renamed)))
                return std::make_pair(at, end);
        }
        if (end == text.size()) break;
        at = end + 1;
    }
    if (table && (table_name == crate || std::regex_search(text.substr(*table), renamed))) return std::make_pair(*table, text.size());
    return std::nullopt;
}

/* Where a version sits in a Cargo entry: a git tag, crate-v as rant writes it or a plain v, a
 * version key, or the entry's own string. */
static std::vector<std::regex> cargo_versions(const Library& lib) {
    return {
        icase(R"((\btag\s*=\s*")((?:crate-)?v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?))"),
        icase(R"((\bversion\s*=\s*"[=^~]?)()(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?))"),
        icase(R"((^\s*"?)" + escaped(lib.names.rust) + R"("?\s*=\s*"[=^~]?)()(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?))"),
    };
}

std::optional<Use> cargo_use(const std::string& text, const Library& lib) {
    auto entry = cargo_entry(text, lib);
    if (!entry) return std::nullopt;
    std::string body = text.substr(entry->first, entry->second - entry->first);
    if (std::regex_search(body, icase(R"(\bworkspace\s*=\s*true\b)"))) {
        Use u = use_of(lib, Kind::Rust, "Cargo workspace", {}, {});
        u.wanted = false;    /* its version is the Cargo workspace's to move */
        return u;
    }
    if (std::regex_search(body, icase(R"(\bpath\s*=)"))) {
        Use u = use_of(lib, Kind::Rust, "Cargo path", {}, {});
        u.wanted = false;
        return u;
    }
    Use u = use_of(lib, Kind::Rust, "Cargo", {}, {});
    for (auto& re : cargo_versions(lib)) {
        std::smatch m;
        if (std::regex_search(body, m, re)) {
            u.version = m[3].str();
            break;
        }
    }
    return u;
}

std::string cargo_set(const std::string& text, const Library& lib, const std::string& version) {
    auto entry = cargo_entry(text, lib);
    if (!entry) throw app::Failure("it has no " + lib.names.rust + " dependency to move");
    std::string body = text.substr(entry->first, entry->second - entry->first);
    if (std::regex_search(body, icase(R"(\bworkspace\s*=\s*true\b)")))
        throw app::Failure("it takes " + lib.name + " from its Cargo workspace, move the version in the workspace Cargo.toml");
    if (std::regex_search(body, icase(R"(\bpath\s*=)"))) throw app::Failure("it uses a " + lib.name + " checkout by path, update that checkout instead");
    for (auto& re : cargo_versions(lib))
        if (std::regex_search(body, re))
            return text.substr(0, entry->first) + with_version(body, re, version) + text.substr(entry->second);
    throw app::Failure("its " + lib.names.rust + " dependency names no version to move, set tag = \"crate-v" + version + "\" by hand");
}

std::string cargo_add(const std::string& text, const Library& lib, const std::string& version) {
    static const std::regex package = icase(R"((^|\n)\s*\[package\])");
    if (!std::regex_search(text, package)) throw app::Failure("it is a Cargo workspace with no package, add " + lib.name + " to a member");
    /* a library's crate-v tag carries what its crate needs packed in, such as Rant's C, so a build needs only cargo */
    std::string line = lib.names.rust + " = { git = \"" + git_url(lib) + "\", tag = \"crate-v" + version + "\" }\n";
    static const std::regex dependencies = icase(R"((^|\n)[ \t]*\[dependencies\][ \t]*(\r?\n|$))");
    std::smatch m;
    if (std::regex_search(text, m, dependencies)) {
        size_t at = m.position(0) + m.length(0);
        std::string head = text.substr(0, at);
        if (at == text.size() && !head.empty() && head.back() != '\n') head += '\n';
        return head + line + text.substr(at);
    }
    std::string out = text;
    if (!out.empty() && out.back() != '\n') out += '\n';
    return out + (out.empty() ? "" : "\n") + "[dependencies]\n" + line;
}

// ---- finding and editing ----

std::vector<Use> in_folder(const fs::path& dir, const std::vector<Library>& libs, bool adding) {
    std::vector<Use> out;
    auto found = [&](std::optional<Use> u, const fs::path& file) {
        if (!u) return;
        u->dir = dir;
        u->file = file;
        out.push_back(*u);
    };
    fs::path cmake = dir / "CMakeLists.txt", cargo = dir / "Cargo.toml";
    std::vector<fs::path> projects;
    std::error_code ec;
    for (auto& e : fs::directory_iterator(dir, ec))
        if (lower(e.path().extension().u8string()) == ".csproj") projects.push_back(e.path());
    std::sort(projects.begin(), projects.end());
    std::vector<const Library*> python;
    for (auto& lib : libs) {
        if (supports(lib, Kind::CMake) && fs::is_regular_file(cmake)) found(cmake_use(read_file(cmake), lib), cmake);
        if (supports(lib, Kind::CSharp))
            for (auto& p : projects) found(csharp_use(read_file(p), lib), p);
        if (supports(lib, Kind::Rust) && fs::is_regular_file(cargo)) found(cargo_use(read_file(cargo), lib), cargo);
        if (supports(lib, Kind::Python)) python.push_back(&lib);
    }
    python_uses(dir, python, adding, out);
    return out;
}

std::vector<Use> under(const fs::path& start, const std::vector<Library>& libs) {
    config::Folders walk = config::folders_under(start);
    std::vector<Use> out;
    auto listed = [&](const fs::path& venv, const std::string& lib) {
        return std::any_of(out.begin(), out.end(), [&](const Use& o) { return o.how == "venv" && o.file == venv && o.library == lib; });
    };
    for (auto& d : walk.folders)
        for (auto& u : in_folder(d, libs)) {
            if (u.how == "venv" && u.venv_exists) {
                if (listed(u.file, u.library)) continue;
                u.dir = u.file.parent_path();
            }
            out.push_back(u);
        }
    for (auto& venv : walk.venvs)
        for (auto& lib : libs) {
            if (!supports(lib, Kind::Python) || listed(venv, lib.name)) continue;
            std::optional<std::string> held = venv_version(venv, lib);
            if (!held) continue;
            Use u = use_of(lib, Kind::Python, "venv", venv.parent_path(), venv);
            u.venv_exists = true;
            u.version = held;
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

void set_version(const fs::path& file, const Library& lib, const std::string& version) {
    std::string text = read_file(file);
    std::string name = lower(file.filename().u8string());
    if (name == "cmakelists.txt") edit(file, cmake_set(text, lib, version), text);
    else if (name == "pyproject.toml") edit(file, pyproject_set(text, lib, version), text);
    else if (lower(file.extension().u8string()) == ".csproj") edit(file, csharp_set(text, lib, version), text);
    else if (name == "cargo.toml") edit(file, cargo_set(text, lib, version), text);
    else throw app::Failure(file.u8string() + " names no " + lib.name + " version rant can move");
}

std::string add(Kind kind, const fs::path& file, const Library& lib, const std::string& version, const std::string& tag) {
    std::string text = read_file(file), hint;
    if (kind == Kind::CMake) edit(file, cmake_add(text, lib, tag, &hint), text);
    else if (kind == Kind::CSharp) edit(file, csharp_add(text, lib, version), text);
    else if (kind == Kind::Rust) edit(file, cargo_add(text, lib, version), text);
    else throw app::Failure("Python gets " + lib.name + " in its venv, not its files");
    return hint;
}

void add_node(const fs::path& cmakelists, const std::string& name, const std::string& source) {
    std::string text = read_file(cmakelists);
    edit(cmakelists, cmake_add_node(text, name, source), text);
}

}
