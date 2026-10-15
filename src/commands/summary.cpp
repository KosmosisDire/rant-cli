#include "commands/summary.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <memory>
#include <thread>

#include "app/failure.hpp"
#include "mesh/client.hpp"
#include "config/config.hpp"
#include "run/logs.hpp"
#include "run/nodes.hpp"
#include "ui/table.hpp"
#include "ui/terminal.hpp"

namespace commands {

using json = nlohmann::ordered_json;

/* Seconds with at most decimals places and no trailing zeros: 2, 0.4, 1.25. */
static std::string seconds(double s, int decimals) {
    char buf[32];
    std::snprintf(buf, sizeof buf, "%.*f", decimals, s);
    std::string t = buf;
    if (t.find('.') != std::string::npos) {
        t.erase(t.find_last_not_of('0') + 1);
        if (t.back() == '.') t.pop_back();
    }
    return t;
}

Summary::Summary(app::Context& ctx, std::string header, std::string params)
    : ctx_(ctx), header_(std::move(header)), params_(std::move(params)) {
    if (!ctx.json && ui::enable_vt(stdout)) live_.emplace();
}

void Summary::add(Row r) {
    rows.push_back(std::move(r));
    show();
}

void Summary::show() {
    if (live_) live_->draw(lines());
}

void Summary::finish() {
    if (ctx_.json) {
        json out = json::array();
        for (auto& r : rows) {
            json j = { { "name", r.name }, { "status", r.status } };
            if (!r.detail.empty()) j["detail"] = r.detail;
            for (auto& [k, v] : r.fields.items()) j[k] = v;
            out.push_back(j);
        }
        ctx_.out.line(json{ { "nodes", out } }.dump(2));
    } else if (live_) {
        live_->draw(lines());
        live_.reset();
    } else {
        for (auto& l : lines()) ctx_.out.line(l);
    }
}

std::string Summary::counts() const {
    std::vector<std::pair<std::string, size_t>> seen;
    for (auto& r : rows) {
        auto it = std::find_if(seen.begin(), seen.end(), [&](auto& s) { return s.first == r.status; });
        if (it == seen.end()) seen.push_back({ r.status, 1 });
        else it->second++;
    }
    std::string out;
    for (auto& [status, n] : seen) out += (out.empty() ? "" : ", ") + std::to_string(n) + " " + status;
    return out;
}

/* The end of an exited node's log, indented under the rows. */
static std::vector<std::string> log_end(app::Context& ctx, const std::string& name, const std::string& log) {
    std::vector<std::string> out = { "  " + name + ", the end of " + ctx.shown(config::from_utf8(log)) + ":" };
    for (auto& l : run::tail(config::from_utf8(log), 10)) out.push_back("    " + ctx.out.paint(ui::Style::Dim, l));
    return out;
}

std::vector<std::string> Summary::lines() const {
    std::vector<std::string> out;
    if (!header_.empty()) {
        out.push_back(ctx_.out.paint(ui::Style::Bold, header_) + (params_.empty() ? "" : "  " + ctx_.out.paint(ui::Style::Dim, params_)));
        out.push_back("");
    }
    size_t digits = 0;
    for (auto& r : rows)
        if (r.pid) digits = std::max(digits, std::to_string(r.pid).size());
    ui::Table t;
    for (auto& r : rows) {
        std::string detail = r.detail;
        if (r.pid) {
            std::string pid = std::to_string(r.pid);
            detail += (detail.empty() ? "" : "   ") + std::string("pid ") + pid + std::string(digits - pid.size() + 3, ' ') +
                      ctx_.shown(config::from_utf8(r.log));
        }
        std::string mark = r.mark == Mark::Running ? ctx_.out.paint(ui::Style::Green, "\xe2\x97\x8f")    /* filled dot */
                         : r.mark == Mark::Stopped ? ctx_.out.paint(ui::Style::Dim, "\xe2\x97\x8b")      /* hollow dot */
                                                   : ctx_.out.paint(ui::Style::Red, "\xe2\x9c\x95");     /* cross */
        t.row({ mark + " " + r.name, r.status, ctx_.out.paint(ui::Style::Dim, detail) });
    }
    for (auto& l : t.lines("  ")) out.push_back(l);
    for (auto& r : rows)
        if (r.mark == Mark::Exited && !r.log.empty()) {
            out.push_back("");
            for (auto& l : log_end(ctx_, r.name, r.log)) out.push_back(l);
        }
    if (!closing.empty()) {
        out.push_back("");
        out.push_back(closing);
    }
    return out;
}

Summary::Row stopped_row(const std::string& name, process::Stopped how) {
    switch (how) {
    case process::Stopped::Gracefully: return { name, Summary::Mark::Stopped, "stopped", "" };
    case process::Stopped::Killed:
        return { name, Summary::Mark::Stopped, "killed", "did not stop within " + std::to_string(run::grace.count()) + " s" };
    case process::Stopped::KilledAtOnce: return { name, Summary::Mark::Stopped, "killed", "Windows cannot ask it to stop" };
    case process::Stopped::AlreadyGone: break;
    }
    return { name, Summary::Mark::Stopped, "already exited", "" };
}

std::string held_by(const std::vector<std::string>& root_keys, const std::string& lead) {
    std::string alone, groups;
    for (auto& key : root_keys) {
        /* "group nav target=red" names nav, "node talker" is talker started by itself */
        if (key.rfind("group ", 0) == 0) groups += (groups.empty() ? "" : ", ") + key.substr(6, key.find(' ', 6) - 6);
        else alone = "started on its own";
    }
    if (groups.empty()) return alone;
    return alone + (alone.empty() ? "" : ", ") + lead + " " + groups;
}

Summary::Row started_row(const state::Instance& rec) {
    Summary::Row r{ rec.name, Summary::Mark::Running, "starting", "", rec.log, rec.tracking.pid };
    r.fields = { { "pid", rec.tracking.pid }, { "log", rec.log } };
    return r;
}

app::OptionSpec wait_option() {
    return { "wait", 0, "S", "seconds to wait for started nodes to come up on the mesh, 10 by default, 0 for none" };
}

app::OptionSpec watch_option() {
    return { "watch", 0, "S", "seconds to watch them once up for an early exit, 2 by default, 0 for none" };
}

static double seconds_option(app::Context& ctx, const char* name, double fallback) {
    auto w = ctx.args.get(name);
    if (!w) return fallback;
    char* end = nullptr;
    double s = std::strtod(w->c_str(), &end);
    if (end == w->c_str() || *end || s < 0) throw app::UsageError(std::string("--") + name + " takes seconds, 0 or more");
    return s;
}

/* The domain a started node joined: its placement's, else the one this command uses. */
static uint16_t domain_of(app::Context& ctx, const state::Instance& rec) {
    auto d = rec.env.find("RANT_DOMAIN");
    return d != rec.env.end() ? (uint16_t)std::atoi(d->second.c_str()) : ctx.domain();
}

int bring_up(app::Context& ctx, Summary& summary, const std::vector<state::Instance>& started, const std::string& group,
             const std::string& verb) {
    using clock = std::chrono::steady_clock;
    using ms = std::chrono::milliseconds;
    double wait = seconds_option(ctx, "wait", 10), span = seconds_option(ctx, "watch", 2);
    auto row = [&](const std::string& name) -> Summary::Row& {
        return *std::find_if(summary.rows.begin(), summary.rows.end(), [&](auto& r) { return r.name == name; });
    };
    auto begin = clock::now();
    auto since = [&] { return std::chrono::duration_cast<ms>(clock::now() - begin); };
    std::map<std::string, ms> exited;    /* by name, how long after the start */
    std::vector<std::string> late;       /* never came up */

    /* wait: a node is up once its name is among its domain's peers */
    std::vector<const state::Instance*> waiting;
    for (auto& rec : started) waiting.push_back(&rec);
    if (wait > 0 && !waiting.empty()) {
        std::map<uint16_t, std::unique_ptr<mesh::Client>> meshes;
        for (auto* rec : waiting) {
            uint16_t d = domain_of(ctx, *rec);
            if (!meshes.count(d)) meshes[d] = std::make_unique<mesh::Client>(d);
        }
        auto end = begin + ms((long long)(wait * 1000));
        while (!waiting.empty()) {
            summary.closing = ctx.out.paint(ui::Style::Dim, "waiting for " + std::to_string(waiting.size()) + " to come up");
            summary.show();
            for (auto it = waiting.begin(); it != waiting.end();) {
                bool up = false;
                for (auto& p : meshes[domain_of(ctx, **it)]->peers()) up |= p.name == (*it)->name;
                if (up) {
                    row((*it)->name).status = verb;
                } else if (!process::alive((*it)->tracking)) {
                    exited[(*it)->name] = since();
                } else {
                    ++it;
                    continue;
                }
                it = waiting.erase(it);
            }
            if (waiting.empty() || clock::now() >= end) break;
            std::this_thread::sleep_for(ms(100));
        }
        for (auto* rec : waiting) late.push_back(rec->name);
    } else {
        for (auto& rec : started) row(rec.name).status = verb;
    }

    /* watch: what came up must stay up a moment */
    std::vector<state::Instance> up;
    for (auto& rec : started)
        if (!exited.count(rec.name) && std::find(late.begin(), late.end(), rec.name) == late.end()) up.push_back(rec);
    std::string counts = summary.counts();
    if (span > 0 && !up.empty()) {
        summary.closing = ctx.out.paint(ui::Style::Dim, "watching them for " + seconds(span, 2) + " s");
        summary.show();
        ms offset = since();
        for (auto& g : run::exited_within(up, ms((long long)(span * 1000)))) exited[g.instance->name] = offset + g.after;
    }

    for (auto& [name, after] : exited) {
        Summary::Row& r = row(name);
        r.mark = Summary::Mark::Exited;
        r.status = "exited after " + seconds(after.count() / 1000.0, 1) + " s";
        r.detail = ctx.shown(config::from_utf8(r.log));
        r.pid = 0;
        r.fields["running"] = false;
    }
    for (auto& name : late) {
        Summary::Row& r = row(name);
        r.mark = Summary::Mark::Exited;
        r.status = "not up after " + seconds(wait, 2) + " s";
        r.detail = ctx.shown(config::from_utf8(r.log));
        r.pid = 0;
        r.fields["running"] = false;
    }
    if (exited.empty() && late.empty()) {
        summary.closing = counts + (span > 0 && !up.empty() ? ", all running after " + seconds(span, 2) + " s" : "");
    } else {
        std::vector<std::string> bad;
        for (auto& [name, _] : exited) bad.push_back(name + " exited");
        for (auto& name : late) bad.push_back(name + " never came up");
        std::string list;
        for (size_t i = 0; i < bad.size(); i++) list += (i == 0 ? "" : i + 1 == bad.size() ? " and " : ", ") + bad[i];
        size_t running = std::count_if(summary.rows.begin(), summary.rows.end(), [](auto& r) { return r.mark == Summary::Mark::Running; });
        size_t failed = exited.size() + late.size();
        std::string target = failed == 1 || group.empty() ? (exited.empty() ? late.front() : exited.begin()->first) : group;
        summary.closing = std::to_string(running) + " of " + std::to_string(summary.rows.size()) + " running, " + list +
                          ". `rant log " + target + "` shows why";
    }
    summary.finish();
    return exited.empty() && late.empty() ? 0 : 1;
}

}
