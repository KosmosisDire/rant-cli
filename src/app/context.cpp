#include "app/context.hpp"

#include "app/failure.hpp"

namespace app {

void Context::open() {
    if (!opened_) opened_ = config::open(cwd);
}

const config::Workspace* Context::workspace() {
    open();
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.warn(d.str());
        opened_->diagnostics.clear();
    }
    return opened_->workspace ? &*opened_->workspace : nullptr;
}

const config::Workspace& Context::require_workspace() {
    open();
    if (!opened_->diagnostics.empty()) {
        for (auto& d : opened_->diagnostics) out.error(d.str());
        throw Failure("");
    }
    if (!opened_->workspace)
        throw Failure("no rant.hcl with a workspace block here or above, run `rant init` to make one");
    return *opened_->workspace;
}

}
