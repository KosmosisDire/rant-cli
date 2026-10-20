#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/plan_output.hpp"
#include "commands/run.hpp"
#include "complete/complete.hpp"
#include "run/logs.hpp"
#include "run/nodes.hpp"
#include "util/interrupt.hpp"

namespace commands {

using json = nlohmann::ordered_json;

/* The nodes a name means: a group's nodes, the running ones or else those its plan names,
 * otherwise the node of that name. */
static std::vector<std::string> nodes_of(app::Context& ctx, const std::string& name) {
    if (!names_group(ctx, name)) return { name };
    std::vector<std::string> out;
    state::State st = run::snapshot(ctx);
    for (auto& r : st.roots)
        if (r.kind == "group" && r.name == name)
            for (auto& i : st.instances)
                if (std::count(i.roots.begin(), i.roots.end(), r.key()) && std::count(out.begin(), out.end(), i.name) == 0)
                    out.push_back(i.name);
    if (!out.empty()) return out;
    config::Plan plan = config::plan_group(ctx.cwd(), name, {});
    require_plan(ctx, plan);
    for (auto& i : plan.instances) out.push_back(i.name);
    return out;
}

static int run(app::Context& ctx) {
    const config::Workspace& ws = ctx.require_scope();
    size_t count = 20;
    if (auto n = ctx.args.get("lines")) {
        char* end = nullptr;
        unsigned long v = std::strtoul(n->c_str(), &end, 10);
        if (end == n->c_str() || *end || n->front() == '-') throw app::UsageError("--lines takes a count, 0 or more");
        count = v;
    }
    bool follow = ctx.args.has("follow");

    std::vector<std::string> names;
    for (auto& w : ctx.args.words)
        for (auto& n : nodes_of(ctx, w))
            if (std::count(names.begin(), names.end(), n) == 0) names.push_back(n);
    if (ctx.args.words.empty()) {
        for (auto& i : run::snapshot(ctx).instances) names.push_back(i.name);
        if (names.empty()) throw app::Failure("nothing rant started is running here, name a node such as `rant log talker`");
    }
    std::vector<config::fs::path> logs;
    for (auto& n : names) {
        logs.push_back(ws.logs / config::from_utf8(n + ".log"));
        std::error_code ec;
        if (!config::fs::is_regular_file(logs.back(), ec)) throw app::Failure("`" + n + "` has no log, see `rant ls nodes`");
    }

    /* several logs interleave, each line led by its node's name */
    size_t width = 0;
    for (auto& n : names) width = std::max(width, n.size());
    auto print = [&](const std::string& name, const std::string& line) {
        if (ctx.json) ctx.out.line(json{ { "node", name }, { "line", line } }.dump(-1, ' ', false, json::error_handler_t::replace));
        else if (names.size() == 1) ctx.out.line(line);
        else ctx.out.line(ctx.out.paint(ui::Style::Dim, name + std::string(width - name.size(), ' ') + " | ") + line);
    };
    std::vector<run::Follower> followers;
    for (size_t i = 0; i < names.size(); i++) {
        if (follow) followers.emplace_back(logs[i]);
        for (auto& l : run::tail(logs[i], count)) print(names[i], l);
    }
    if (!follow) return 0;

    util::catch_interrupt();
    while (!util::interrupted()) {
        for (size_t i = 0; i < names.size(); i++)
            for (auto& l : followers[i].poll()) print(names[i], l);
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    std::vector<std::string> out = r.groups();
    for (auto& n : r.logged_nodes()) out.push_back(n);
    return { out };
}

app::Command log() {
    app::Command c{ "log", "[group | node...]", "show what nodes wrote, every running node when nothing is named",
                    app::Section::Workspace,
                    { { "follow", 'f', "", "keep printing new lines until Ctrl-C" },
                      { "lines", 'n', "N", "lines of each log to show first, 20 by default" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
