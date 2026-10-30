#include "net/github.hpp"

#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <map>
#include <optional>
#include <sstream>
#include <tuple>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "config/config.hpp"
#include "process/command.hpp"
#include "util/home.hpp"

namespace net {

using json = nlohmann::json;

const Asset* Release::asset(const std::string& name) const {
    for (auto& a : assets)
        if (a.name == name) return &a;
    return nullptr;
}

static std::string read_file(const fs::path& p) {
    std::ifstream in(p, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

/* A scratch file for one curl run, unique to this call, removed by its owner. */
struct Scratch {
    fs::path path;
    explicit Scratch(const std::string& what) {
        static unsigned made = 0;
        fs::path dir = util::rant_home() / "cache";
        std::error_code ec;
        fs::create_directories(dir, ec);
        path = dir / (what + "-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "-" +
                      std::to_string(made++));
    }
    ~Scratch() {
        std::error_code ec;
        fs::remove(path, ec);
    }
};

static void need_curl() {
    if (!process::find_program("curl", fs::temp_directory_path())) throw app::Failure("downloading needs curl, which is not on PATH");
}

static std::string host(const std::string& url) {
    size_t start = url.find("://");
    start = start == std::string::npos ? 0 : start + 3;
    return url.substr(start, url.find('/', start) - start);
}

/* What a failed curl run means, from its exit code. */
static std::string curl_failure(const std::string& url, int code) {
    switch (code) {
    case 5:
    case 6:  return "cannot reach " + host(url) + ", its name did not resolve";
    case 7:  return "cannot reach " + host(url);
    case 28: return "cannot reach " + host(url) + " in time";
    case 22: return "downloading " + url + " failed, the server answered with an error";
    default: return "curl could not fetch " + url + " (exit " + std::to_string(code) + ")";
    }
}

std::string platform() {
#if defined(_WIN32) && defined(_M_ARM64)
    return "win-arm64";
#elif defined(_WIN32)
    return "win-x64";
#elif defined(__APPLE__) && defined(__aarch64__)
    return "osx-arm64";
#elif defined(__APPLE__)
    return "osx-x64";
#elif defined(__aarch64__)
    return "linux-arm64";
#else
    return "linux-x64";
#endif
}

std::string exe_suffix() {
#ifdef _WIN32
    return ".exe";
#else
    return "";
#endif
}

/* A version as a tag writes it, its numbers then the stage of a pre-release, compared in
 * this order. */
struct Version {
    std::vector<long> numbers;      /* trailing zeros dropped, so 1.2 is 1.2.0 */
    int               stage = 4;    /* dev, alpha, beta, rc, the release, then post */
    long              n = 0;        /* the number after the stage, as in beta.2 */
    std::string       rest;
};

static Version parse_version(std::string v) {
    if (!v.empty() && (v[0] == 'v' || v[0] == 'V')) v.erase(0, 1);
    v = v.substr(0, v.find('+'));    /* build metadata orders nothing */
    Version out;
    size_t at = 0;
    while (at < v.size() && std::isdigit(static_cast<unsigned char>(v[at]))) {
        size_t end = at;
        while (end < v.size() && std::isdigit(static_cast<unsigned char>(v[end]))) end++;
        out.numbers.push_back(std::strtol(v.substr(at, end - at).c_str(), nullptr, 10));
        at = end;
        if (at + 1 < v.size() && v[at] == '.' && std::isdigit(static_cast<unsigned char>(v[at + 1]))) at++;
        else break;
    }
    while (!out.numbers.empty() && out.numbers.back() == 0) out.numbers.pop_back();
    /* -beta.1, -rc.1 and PEP 440's b1 all read as a word and a number */
    std::string tag;
    for (char c : v.substr(at))
        if (c != '-' && c != '.' && c != '_') tag += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    if (tag.empty()) return out;
    size_t letters = 0;
    while (letters < tag.size() && std::isalpha(static_cast<unsigned char>(tag[letters]))) letters++;
    static const std::map<std::string, int> stages = { { "dev", 0 },  { "a", 1 },  { "alpha", 1 }, { "b", 2 },       { "beta", 2 },
                                                       { "c", 3 },    { "rc", 3 }, { "pre", 3 },   { "preview", 3 }, { "post", 5 } };
    auto it = stages.find(tag.substr(0, letters));
    out.stage = it == stages.end() ? 1 : it->second;
    char* end = nullptr;
    out.n = std::strtol(tag.c_str() + letters, &end, 10);
    out.rest = end;
    return out;
}

bool newer(const std::string& version, const std::string& than) {
    Version a = parse_version(version), b = parse_version(than);
    return std::tie(a.numbers, a.stage, a.n, a.rest) > std::tie(b.numbers, b.stage, b.n, b.rest);
}

/* A tag's version: the tag without a v in front of its first digit. */
static std::string version_of(const std::string& tag) {
    bool v = tag.size() > 1 && (tag[0] == 'v' || tag[0] == 'V') && std::isdigit(static_cast<unsigned char>(tag[1]));
    return v ? tag.substr(1) : tag;
}

Release parse_release(const std::string& body) {
    json j = json::parse(body, nullptr, false);
    if (!j.is_object() || !j.value("tag_name", json()).is_string()) throw app::Failure("cannot read the release from GitHub");
    Release r;
    r.tag = j["tag_name"].get<std::string>();
    r.version = version_of(r.tag);
    for (auto& a : j.value("assets", json::array()))
        if (a.is_object() && a.value("name", json()).is_string() && a.value("browser_download_url", json()).is_string())
            r.assets.push_back({ a["name"].get<std::string>(), a["browser_download_url"].get<std::string>() });
    return r;
}

/* Fetches each url into its body file and returns each HTTP status, with the token when given
 * one. One curl run fetches them all over one connection. */
static std::vector<std::string> get(const std::vector<std::pair<std::string, fs::path>>& urls, bool api) {
    need_curl();
    Scratch status("status"), headers("headers");
    std::vector<std::string> argv = { "curl", "-sS", "-L", "--max-time", "60", "-A", "rant-cli", "-w", "%{http_code}\n" };
    if (api) argv.insert(argv.end(), { "-H", "Accept: application/vnd.github+json" });
    /* the token goes through a file, never the command line other users can read */
    if (const char* token = std::getenv("GITHUB_TOKEN"); api && token && *token) {
        std::ofstream(headers.path, std::ios::binary) << "Authorization: Bearer " << token << "\n";
        argv.insert(argv.end(), { "-H", "@" + config::to_utf8(headers.path) });
    }
    for (auto& [url, body] : urls) argv.insert(argv.end(), { "-o", config::to_utf8(body), url });
    int code = process::run({ argv, fs::temp_directory_path(), {} }, status.path);
    std::vector<std::string> out;
    std::istringstream lines(read_file(status.path));
    for (std::string line; std::getline(lines, line);) out.push_back(line.substr(0, line.find('\r')));
    /* a transfer that reached no server shows the status 000 */
    for (size_t i = 0; i < urls.size(); i++)
        if (i >= out.size() || out[i] == "000") throw app::Failure(curl_failure(urls[i].first, code ? code : 7));
    if (code != 0) throw app::Failure(curl_failure(urls[0].first, code));
    return out;
}

/* One release by the API's path for it, nullopt when there is none. */
static std::optional<Release> release_at(const std::string& repo, const std::string& path) {
    std::string url = "https://api.github.com/repos/" + repo + "/releases/" + path;
    Scratch body("release");
    std::string http = get({ { url, body.path } }, true)[0];
    if (http == "200") return parse_release(read_file(body.path));
    if (http == "404") return std::nullopt;
    if (http == "403" || http == "429") throw app::Failure("GitHub refused, its rate limit was probably reached. Wait an hour or set GITHUB_TOKEN");
    throw app::Failure("GitHub answered " + http + " for " + url);
}

Release release(const std::string& repo, const std::string& version) {
    if (version.empty()) {
        if (auto r = release_at(repo, "latest")) return *r;
        throw app::Failure(repo + " has no release yet");
    }
    std::string bare = version_of(version);
    for (auto& tag : { "v" + bare, bare })
        if (auto r = release_at(repo, "tags/" + tag)) return *r;
    throw app::Failure(repo + " has no release " + bare);
}

std::vector<std::optional<std::string>> fetch_all(const std::vector<std::string>& urls) {
    std::vector<std::optional<std::string>> out;
    if (urls.empty()) return out;
    std::vector<Scratch> bodies;
    bodies.reserve(urls.size());
    std::vector<std::pair<std::string, fs::path>> pairs;
    for (auto& url : urls) pairs.push_back({ url, bodies.emplace_back("fetch").path });
    std::vector<std::string> http = get(pairs, false);
    for (size_t i = 0; i < urls.size(); i++) {
        if (http[i] == "200") out.push_back(read_file(bodies[i].path));
        else if (http[i] == "404") out.push_back(std::nullopt);
        else throw app::Failure(host(urls[i]) + " answered " + http[i] + " for " + urls[i]);
    }
    return out;
}

std::optional<std::string> fetch(const std::string& url) { return fetch_all({ url })[0]; }

std::string raw_url(const std::string& repo, const std::string& ref, const std::string& file) {
    return "https://raw.githubusercontent.com/" + repo + "/" + ref + "/" + file;
}

void download(const std::string& url, const fs::path& dest, bool program) {
    need_curl();
    std::error_code ec;
    fs::create_directories(dest.parent_path(), ec);
    fs::path part = dest;
    part += ".part";
    int code = process::run({ { "curl", "-fsSL", "--retry", "2", "-A", "rant-cli", "-o", config::to_utf8(part), url }, fs::temp_directory_path(), {} });
    if (code != 0) {
        fs::remove(part, ec);
        throw app::Failure(curl_failure(url, code));
    }
    if (program) fs::permissions(part, fs::perms::owner_exec | fs::perms::group_exec | fs::perms::others_exec, fs::perm_options::add, ec);
    fs::rename(part, dest, ec);
    if (!ec) return;
    fs::path old = dest;
    old += ".old";
    fs::remove(old, ec);
    fs::rename(dest, old, ec);
    fs::rename(part, dest, ec);
    if (ec) {
        fs::remove(part, ec);
        throw app::Failure("cannot replace " + dest.u8string() + ": " + ec.message());
    }
}

}
