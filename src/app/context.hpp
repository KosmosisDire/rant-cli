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
    uint16_t   domain = 0;      /* --domain */
    std::filesystem::path cwd = std::filesystem::current_path();

    /* The enclosing workspace, or nullptr when there is none. A broken config is reported
     * as a warning, since mesh commands still work without it. */
    const config::Workspace* workspace();

    /* The enclosing workspace, or a failure that says to run rant init. With packages it
     * also holds every package and node type, and any config error is a failure. */
    const config::Workspace& require_workspace(bool packages = false);

private:
    void open(bool packages);
    std::optional<config::Opened> opened_;
    bool with_packages_ = false;
};

}
