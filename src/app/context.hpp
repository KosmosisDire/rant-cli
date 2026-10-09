#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>

#include "app/args.hpp"
#include "config/config.hpp"
#include "ui/output.hpp"

namespace app {

/* What every command gets: the parsed arguments, the output, the global options and the
 * workspace, opened on first use. */
class Context {
public:
    ui::Output out;
    Args       args;
    bool       yes = false;     /* -y: answer every question yes */
    bool       json = false;    /* --json: print JSON on stdout */
    std::optional<uint16_t> domain_option;    /* --domain */

    /* The working directory, or a failure when it was deleted under this shell. */
    const std::filesystem::path& cwd() const;

    /* A path for messages: relative to the working directory, "." for it. */
    std::string shown(const std::filesystem::path& p) const;

    /* The domain mesh commands use: --domain, else the workspace's, else RANT_DOMAIN, else 0.
     * Nodes rant starts take the same order from their config. */
    uint16_t domain();

    /* The enclosing workspace, or nullptr when there is none. A broken config is reported
     * as a warning, since mesh commands still work without it. */
    const config::Workspace* workspace();

    /* The enclosing workspace, or a failure that says to run rant init. With packages it
     * also holds every package and node type, and any config error is a failure. */
    const config::Workspace& require_workspace(bool packages = false);

private:
    void open(bool packages);
    static std::filesystem::path here();
    std::filesystem::path cwd_ = here();
    std::optional<config::Opened> opened_;
    bool with_packages_ = false;
};

}
