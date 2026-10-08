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

    /* The enclosing workspace, or a failure that says to run rant init. */
    const config::Workspace& require_workspace();

private:
    void open();
    std::optional<config::Opened> opened_;
};

}
