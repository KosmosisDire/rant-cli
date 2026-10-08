#include "app/context.hpp"

#include "app/failure.hpp"

namespace app {

void Context::open(bool packages) {
    if (opened_ && (with_packages_ || !packages)) return;
    opened_ = config::open(cwd, packages);
    with_packages_ = packages;
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
