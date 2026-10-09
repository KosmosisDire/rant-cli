#include "app/context.hpp"

#include <cstdlib>

#include "app/failure.hpp"

namespace app {

std::filesystem::path Context::here() {
    std::error_code ec;
    auto p = std::filesystem::current_path(ec);
    return ec ? std::filesystem::path() : p;
}

const std::filesystem::path& Context::cwd() const {
    if (cwd_.empty()) throw Failure("this folder no longer exists, cd into one that does");
    return cwd_;
}

void Context::open(bool packages) {
    if (opened_ && (with_packages_ || !packages)) return;
    opened_ = config::open(cwd(), packages);
    with_packages_ = packages;
}

std::string Context::shown(const std::filesystem::path& p) const {
    std::error_code ec;
    std::filesystem::path rel = std::filesystem::relative(p, cwd_, ec);
    if (ec || rel.empty()) return config::to_utf8(p);
    return config::to_utf8(rel);
}

uint16_t Context::domain() {
    if (domain_option) return *domain_option;
    /* a broken config is left for the command to report */
    open(false);
    if (opened_->workspace && opened_->workspace->domain) return *opened_->workspace->domain;
    const char* env = std::getenv("RANT_DOMAIN");
    if (!env || !*env) return 0;
    char* end = nullptr;
    unsigned long d = std::strtoul(env, &end, 10);
    if (*end || d > 65535) throw Failure("RANT_DOMAIN is not a number from 0 to 65535");
    return static_cast<uint16_t>(d);
}

const config::Workspace* Context::workspace() {
    open(false);
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.warn(d.str());
        opened_->diagnostics.clear();
    }
    return opened_->workspace ? &*opened_->workspace : nullptr;
}

const config::Workspace& Context::require_workspace(bool packages) {
    open(packages);
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.error(d.str());
        throw Failure("");
    }
    if (!opened_->workspace)
        throw Failure("no rant.hcl with a workspace block here or above, run `rant init` to make one");
    return *opened_->workspace;
}

}
