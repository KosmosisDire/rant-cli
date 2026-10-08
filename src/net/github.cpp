#include "net/github.hpp"

#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iterator>

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
        fs::path dir = util::rant_home() / "cache";
        std::error_code ec;
        fs::create_directories(dir, ec);
        path = dir / (what + "-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    }
    ~Scratch() {
        std::error_code ec;
        fs::remove(path, ec);
    }
};

static void need_curl() {
    if (!process::find_program("curl", fs::current_path())) throw app::Failure("downloading needs curl, which is not on PATH");
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

Release parse_release(const std::string& body) {
    json j = json::parse(body, nullptr, false);
    if (!j.is_object() || !j.value("tag_name", json()).is_string()) throw app::Failure("GitHub answered a release that cannot be read");
    Release r;
    r.tag = j["tag_name"].get<std::string>();
    r.version = r.tag.rfind('v', 0) == 0 ? r.tag.substr(1) : r.tag;
    for (auto& a : j.value("assets", json::array()))
        if (a.is_object() && a.value("name", json()).is_string() && a.value("browser_download_url", json()).is_string())
            r.assets.push_back({ a["name"].get<std::string>(), a["browser_download_url"].get<std::string>() });
    return r;
}

Release release(const std::string& repo, const std::string& tag) {
    need_curl();
    std::string t = tag.empty() || tag[0] == 'v' ? tag : "v" + tag;
    std::string url = "https://api.github.com/repos/" + repo + "/releases/" + (t.empty() ? "latest" : "tags/" + t);
    Scratch body("release"), status("status"), headers("headers");
    std::vector<std::string> argv = { "curl", "-sS", "-L", "--max-time", "60", "-H", "Accept: application/vnd.github+json",
                                      "-A", "rant-cli", "-o", config::to_utf8(body.path), "-w", "%{http_code}" };
    /* the token goes through a file, never the command line other users can read */
    if (const char* token = std::getenv("GITHUB_TOKEN"); token && *token) {
        std::ofstream(headers.path, std::ios::binary) << "Authorization: Bearer " << token << "\n";
        argv.insert(argv.end(), { "-H", "@" + config::to_utf8(headers.path) });
    }
    argv.push_back(url);
    int code = process::run({ argv, fs::current_path(), {} }, status.path);
    if (code != 0) throw app::Failure(curl_failure(url, code));
    std::string http = read_file(status.path);
    if (http == "200") return parse_release(read_file(body.path));
    if (http == "404") throw app::Failure(t.empty() ? repo + " has no release yet" : repo + " has no release " + t);
    if (http == "403" || http == "429") throw app::Failure("GitHub refused, its rate limit was probably reached. Wait an hour or set GITHUB_TOKEN");
    throw app::Failure("GitHub answered " + http + " for " + url);
}

void download(const std::string& url, const fs::path& dest) {
    need_curl();
    std::error_code ec;
    fs::create_directories(dest.parent_path(), ec);
    fs::path part = dest;
    part += ".part";
    int code = process::run({ { "curl", "-fsSL", "--retry", "2", "-A", "rant-cli", "-o", config::to_utf8(part), url }, fs::current_path(), {} });
    if (code != 0) {
        fs::remove(part, ec);
        throw app::Failure(curl_failure(url, code));
    }
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
