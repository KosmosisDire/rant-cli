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
    if (cwd_.empty()) throw Failure("this folder was deleted, cd elsewhere");
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
    std::string whole = config::to_utf8(p);
    if (ec || rel.empty()) return whole;
    /* a path far from here reads better whole than as a long climb */
    std::string near = config::to_utf8(rel);
    return near.size() <= whole.size() ? near : whole;
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
    if (*end || d > 65535) throw Failure("RANT_DOMAIN `" + std::string(env) + "` is not a number from 0 to 65535");
    return static_cast<uint16_t>(d);
}

const config::Workspace* Context::workspace() {
    const config::Workspace* ws = scope();
    return ws && !ws->home ? ws : nullptr;
}

const config::Workspace& Context::require_workspace(bool packages) {
    const config::Workspace& ws = require_scope(packages);
    if (ws.home) throw Failure("no workspace here, `rant init` makes one");
    return ws;
}

const config::Workspace* Context::scope() {
    open(false);
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.warn(d.str());
        opened_->diagnostics.clear();
    }
    return opened_->workspace ? &*opened_->workspace : nullptr;
}

const config::Workspace& Context::require_scope(bool packages) {
    open(packages);
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.error(d.str());
        throw Failure("");
    }
    if (!opened_->workspace) throw Failure("no workspace here, `rant init` makes one");
    return *opened_->workspace;
}

}
