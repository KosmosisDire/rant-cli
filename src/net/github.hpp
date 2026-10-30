#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

/* GitHub releases and downloads, through the system's curl, so the system's own name
 * lookups and certificates apply. GITHUB_TOKEN, when set, lifts the API rate limit. */
namespace net {

namespace fs = std::filesystem;

struct Asset {
    std::string name;
    std::string url;
};

struct Release {
    std::string        tag;
    std::string        version;    /* the tag without a v before its number */
    std::vector<Asset> assets;

    const Asset* asset(const std::string& name) const;
};

/* A release of owner/name: the latest that is no pre-release when version is empty, else the
 * one tagged v<version> or <version>, given with or without its v. Throws app::Failure. */
Release release(const std::string& repo, const std::string& version = "");

/* Whether a version is newer than another, number by number, 0.0.12 over 0.0.9, and a
 * pre-release such as 1.2.0-beta.1, 1.2.0-rc.1 or 1.2.0b1 before its release. */
bool newer(const std::string& version, const std::string& than);

/* A release from the API's JSON answer. Throws app::Failure when it cannot be read. */
Release parse_release(const std::string& body);

/* Downloads url to dest, which is whole or untouched afterwards, and with program marks it
 * runnable. A running program on Windows cannot be replaced but can be renamed, so the old
 * file steps aside first. Throws app::Failure. */
void download(const std::string& url, const fs::path& dest, bool program = false);

/* The text of a file at url, nullopt when the server has none there. Throws app::Failure
 * when it cannot be fetched. */
std::optional<std::string> fetch(const std::string& url);

/* The text at each url, as fetch gives it, fetched together over one connection. */
std::vector<std::optional<std::string>> fetch_all(const std::vector<std::string>& urls);

/* The address of one file of a GitHub repo at a tag or branch, "HEAD" for the default branch. */
std::string raw_url(const std::string& repo, const std::string& ref, const std::string& file);

/* The platform this runs on as release files name it: "win-x64", "linux-x64", "osx-arm64". */
std::string platform();

/* What a program's file name ends with here: ".exe" on Windows, else nothing. */
std::string exe_suffix();

}
