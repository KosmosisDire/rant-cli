#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "complete/complete.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "ui/live.hpp"
#include "util/interrupt.hpp"

namespace commands {

using mesh::json;
using Clock = std::chrono::steady_clock;

/* One value as it came, kept as bytes, since a message view is gone at the next take. */
struct Sample {
    std::vector<uint8_t> data;
    rant::Schema         schema;
    std::string          from;

    json value() const { return mesh::to_json(rant::Bytes(data.data(), data.size()), schema); }
};

/* The next value of a topic or a variable, or nothing within wait. */
using Source = std::function<std::optional<Sample>(std::chrono::milliseconds wait)>;

static Source topic_source(rant::Subscriber<rant::Bytes>& sub) {
    return [&sub](std::chrono::milliseconds wait) -> std::optional<Sample> {
        auto m = sub.take(wait);
        if (!m) return std::nullopt;
        return Sample{ { m->data().data(), m->data().data() + m->data().size() }, rant::Schema(m->raw_schema()),
                       std::string(m->publisher_name()) };
    };
}

/* The node that owns a variable, as a record names its sender. */
static std::string owner_of(mesh::Client& mesh, const rant::Entity& e) {
    for (auto& p : mesh.peers())
        if (p.id == e.provider) return p.name;
    return e.from;
}

/* A variable's changes, the current value first. The queue is shared with the handler, which
 * lives until the node closes. */
static Source variable_source(rant::RemoteVariable<rant::Bytes>& var, const std::string& owner) {
    struct Queue {
        std::mutex              mu;
        std::condition_variable cv;
        std::deque<Sample>      samples;
    };
    auto q = std::make_shared<Queue>();
    var.on_change([q, owner](const rant::VariableUpdate& u) {
        std::lock_guard<std::mutex> g(q->mu);
        q->samples.push_back({ { u.value().data(), u.value().data() + u.value().size() }, rant::Schema(u.raw_schema()), owner });
        q->cv.notify_one();
    });
    return [q](std::chrono::milliseconds wait) -> std::optional<Sample> {
        std::unique_lock<std::mutex> g(q->mu);
        if (!q->cv.wait_for(g, wait, [&] { return !q->samples.empty(); })) return std::nullopt;
        Sample s = std::move(q->samples.front());
        q->samples.pop_front();
        return s;
    };
}

/* Values a second over the last two seconds, measured across the span the values cover, so
 * it is right from the second value on. */
class Rate {
public:
    void hit(Clock::time_point t) { times_.push_back(t); }
    double hz(Clock::time_point now) {
        while (!times_.empty() && now - times_.front() > std::chrono::seconds(2)) times_.pop_front();
        if (times_.size() < 2) return 0;
        double span = std::chrono::duration<double>(times_.back() - times_.front()).count();
        return span > 0 ? (times_.size() - 1) / span : 0;
    }

private:
    std::deque<Clock::time_point> times_;
};

static std::string fixed(double v, int decimals) {
    char buf[32];
    std::snprintf(buf, sizeof buf, "%.*f", decimals, v);
    return buf;
}

/* The newest value redrawn in place, under a line with who sends it and how fast. */
static void live(app::Context& ctx, const Source& next, const std::string& name, long count) {
    ui::LiveView view;
    Printer printer(format_of(ctx), &ctx.out, true);
    std::optional<Sample> latest;
    Rate rate;
    long seen = 0;
    Clock::time_point last_msg{}, last_draw{};
    bool dirty = true;
    while (!util::interrupted() && (count == 0 || seen < count)) {
        auto m = next(std::chrono::milliseconds(50));
        while (m) {    /* every waiting value counts, the newest is shown */
            seen++;
            last_msg = Clock::now();
            rate.hit(last_msg);
            latest = std::move(m);
            dirty = true;
            if (count && seen >= count) break;
            m = next(std::chrono::milliseconds(0));
        }
        auto now = Clock::now();
        /* a new value redraws at 30 frames a second at most, and the rate and age keep
           moving between values at 4 */
        bool done = count && seen >= count;
        bool due = (dirty && now - last_draw >= std::chrono::milliseconds(33)) ||
                   now - last_draw >= std::chrono::milliseconds(250);
        if (!due && !done) continue;

        std::string status;
        if (!latest) {
            status = "waiting for a message";
        } else {
            double age = std::chrono::duration<double>(now - last_msg).count();
            std::string hz = fixed(rate.hz(now), 1);
            hz.insert(0, hz.size() < 6 ? 6 - hz.size() : 0, ' ');    /* the header holds still too */
            status = "from " + latest->from + "  " + hz + " Hz  " + std::to_string(seen) +
                     (seen == 1 ? " message" : " messages");
            if (age > 1) status += "  last " + fixed(age, 1) + " s ago";
        }
        std::vector<std::string> lines{ ctx.out.paint(ui::Style::Bold, name) + "  " + ctx.out.paint(ui::Style::Faint, status) };
        if (latest) {
            std::string body = printer.block(latest->value());
            size_t start = 0;
            while (start <= body.size()) {
                size_t end = body.find('\n', start);
                if (end == std::string::npos) end = body.size();
                lines.push_back(body.substr(start, end - start));
                start = end + 1;
            }
        }
        view.draw(lines);
        last_draw = now;
        dirty = false;
    }
}

/* A record per value: who sent it, the rate in values a second, and the value. */
static void stream(app::Context& ctx, const Source& next, long count) {
    Printer printer(format_of(ctx), &ctx.out, true);
    Rate rate;
    long seen = 0;
    while (!util::interrupted() && (count == 0 || seen < count)) {
        auto m = next(std::chrono::milliseconds(100));
        if (!m) continue;
        auto now = Clock::now();
        rate.hit(now);
        ctx.out.line(printer.line(json{ { "from", m->from }, { "hz", std::round(rate.hz(now) * 10) / 10 }, { "value", m->value() } }));
        std::fflush(stdout);
        seen++;
    }
}

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("sub takes one topic or variable");
    const std::string& name = ctx.args.words[0];
    long count = 0;
    if (auto c = ctx.args.get("count")) {
        count = std::strtol(c->c_str(), nullptr, 10);
        if (count <= 0) throw app::UsageError("--count takes a positive number");
    }
    format_of(ctx);
    bool live_view = live_of(ctx);

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    std::optional<rant::Subscriber<rant::Bytes>> sub;
    std::optional<rant::RemoteVariable<rant::Bytes>> var;
    Source next;
    if (auto e = mesh.node().reflection().find(rant::EntityKind::Variable, name)) {
        rant::VariableOptions<rant::Bytes> o;
        o.reflect_from_mesh = true;
        var.emplace(mesh.node().remote_variable<rant::Bytes>(name, o));
        next = variable_source(*var, owner_of(mesh, *e));
    } else {
        if (!mesh.node().reflection().find(rant::EntityKind::Topic, name))
            ctx.out.note("no publisher of `" + name + "` yet, waiting for one");
        rant::Qos qos;
        qos.reflect_from_mesh = true;
        sub.emplace(mesh.node().subscriber<rant::Bytes>(name, qos));
        next = topic_source(*sub);
    }
    if (live_view) live(ctx, next, name, count);
    else stream(ctx, next, count);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Topic, rant::EntityKind::Variable }), true };
}

app::Command sub() {
    app::Command c{ "sub", "<topic|variable>", "show the newest value of a topic or variable until Ctrl-C", app::Section::Mesh,
                    { { "count", 'n', "N", "stop after N values" }, lines_option(), csv_option() }, run };
    c.complete = complete_words;
    return c;
}

}
