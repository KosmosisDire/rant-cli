#pragma once

#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "app/args.hpp"
#include "app/context.hpp"
#include "process/supervisor.hpp"
#include "state/state.hpp"
#include "ui/live.hpp"

namespace commands {

/* What run, stop or restart did to nodes, as one block: a header naming the group, a row
 * per node and a closing line. On a terminal it is redrawn as rows come, elsewhere printed
 * once at the end. With --json the rows print as objects instead. */
class Summary {
public:
    enum class Mark { Running, Stopped, Exited };

    struct Row {
        std::string name;
        Mark        mark = Mark::Running;
        std::string status;    /* "started", "stopped", "already running" */
        std::string detail;    /* dim, after the status */
        std::string log;       /* the log whose end shows below an exited node */
        uint64_t    pid = 0;   /* shown with the log, both lined up, while it runs */
        nlohmann::ordered_json fields = nlohmann::ordered_json::object();    /* more for --json */
    };

    /* header is the group, empty for nodes on their own. params follow it, dim. */
    Summary(app::Context& ctx, std::string header, std::string params = "");

    std::vector<Row> rows;
    std::string      closing;

    void add(Row r);
    void show();
    void finish();

    /* The rows counted by status in the order they first come: "2 stopped, 1 kept running". */
    std::string counts() const;

private:
    std::vector<std::string> lines() const;

    app::Context&               ctx_;
    std::string                 header_, params_;
    std::optional<ui::LiveView> live_;
};

/* A stopped node's row, saying how it stopped. */
Summary::Row stopped_row(const std::string& name, process::Stopped how);

/* Why a node other roots hold stays up: "started on its own" for its own start, then lead
 * and the groups, such as "shared with nav" or "still needed by nav". */
std::string held_by(const std::vector<std::string>& root_keys, const std::string& lead);

/* A started node's row: its pid and log, and status "starting" until it comes up. */
Summary::Row started_row(const state::Instance& rec);

/* The --wait and --watch options of run and restart. */
app::OptionSpec wait_option();
app::OptionSpec watch_option();

/* Hands the nodes with restart set to the service process, once the workspace's state is
 * unlocked. Then waits up to --wait seconds, 10 by default, for each node just started to
 * show on the mesh, and watches them --watch seconds more, 2 by default, since a node that
 * crashes at once, on an import error say, otherwise looks started. A node that comes up
 * takes the status verb, one that exits or never comes up is marked with the end of its log.
 * Closes the summary and prints it, and returns 1 when a node failed. group names what
 * `rant log` should show when several failed. */
int bring_up(app::Context& ctx, Summary& summary, const config::Workspace& ws, std::vector<state::Instance> started,
             const std::string& group, const std::string& verb);

}
