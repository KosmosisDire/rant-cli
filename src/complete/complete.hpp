#pragma once

#include <chrono>
#include <initializer_list>
#include <optional>
#include <string>
#include <vector>

#include "app/context.hpp"
#include "config/config.hpp"
#include "mesh/snapshot.hpp"

/* Tab completion, answered by the binary for every shell. A shell hook runs
 * `rant __complete --cur=<word at the cursor> <words before it...>` and offers each line
 * printed. A candidate ending in / or = takes no space after it. */
namespace complete {

/* The mesh query one completion makes: whoever answers within this joins the snapshot. */
inline constexpr std::chrono::milliseconds mesh_budget{ 80 };

/* The words a command offers at the cursor. Entity paths go one segment at a time, like
 * file paths, so /cam offers /camera/ before /camera/left. */
struct Candidates {
    std::vector<std::string> words;
    bool                     paths = false;
};

/* One request as a command's completer sees it. Each source loads on first use, so a
 * completion that needs no mesh never touches it. */
class Request {
public:
    Request(app::Context& ctx, std::vector<std::string> words, std::string partial);

    const std::vector<std::string> words;      /* after the command, options left out */
    const std::string              partial;    /* the word at the cursor, maybe empty */

    std::vector<std::string> node_types();     /* package/name, and the bare name when unique */
    std::vector<std::string> groups();
    std::vector<std::string> packages();
    config::GroupInfo        group(const std::string& name);
    std::vector<std::string> running_nodes();  /* the instances rant started */
    std::vector<std::string> running_groups();
    std::vector<std::string> mesh_nodes();
    std::vector<std::string> entities(std::initializer_list<rant::EntityKind> kinds = {});

private:
    const config::Workspace* workspace();
    const mesh::Snapshot&    mesh();

    app::Context&                 ctx_;
    std::optional<config::Opened> opened_;
    std::optional<mesh::Snapshot> mesh_;
};

/* `key=` for each param not yet given, or `key=value` for the options of the one being typed. */
std::vector<std::string> params(const config::GroupInfo& group, const std::string& partial,
                                const std::vector<std::string>& given);

/* The candidates that start with partial, each path cut after the next slash past it,
 * sorted and without repeats. */
std::vector<std::string> narrow(const Candidates& c, const std::string& partial);

/* Answers one __complete request on stdout. Never fails: a broken workspace or a silent
 * mesh only means fewer candidates. */
int run(app::Context& ctx, const std::vector<std::string>& tokens);

}
