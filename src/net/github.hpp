#pragma once

#include <filesystem>
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
    std::string        version;    /* the tag without its v */
    std::vector<Asset> assets;

    const Asset* asset(const std::string& name) const;
};

/* A release of owner/name, the latest when tag is empty. A version without its v finds the
 * v tag too. Throws app::Failure. */
Release release(const std::string& repo, const std::string& tag = "");

/* A release from the API's JSON answer. Throws app::Failure when it cannot be read. */
Release parse_release(const std::string& body);

/* Downloads url to dest, which is whole or untouched afterwards. A running program on
 * Windows cannot be replaced but can be renamed, so the old file steps aside first.
 * Throws app::Failure. */
void download(const std::string& url, const fs::path& dest);

}
