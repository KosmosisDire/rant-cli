#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <deque>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "complete/complete.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "ui/csv.hpp"
#include "ui/live.hpp"
#include "ui/terminal.hpp"
#include "ui/yaml.hpp"
#include "util/interrupt.hpp"

namespace commands {

using mesh::json;
using Clock = std::chrono::steady_clock;

/* The newest message, kept as bytes, since a view is gone at the next take. */
struct Latest {
    std::vector<uint8_t> data;
    rant::Schema         schema;
    std::string          from;
    bool                 any = false;
};

/* Messages a second over the last two seconds, measured across the span the messages
 * cover, so it is right from the second message on. */
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
static void live(app::Context& ctx, rant::Subscriber<rant::Bytes>& sub, const std::string& topic, long count) {
    ui::LiveView view;
    ui::Yaml yaml(&ctx.out, true);
    Latest latest;
    Rate rate;
    long seen = 0;
    Clock::time_point last_msg{}, last_draw{};
    bool dirty = true;
    while (!util::interrupted() && (count == 0 || seen < count)) {
        auto m = sub.take(std::chrono::milliseconds(50));
        while (m) {    /* every waiting message counts, the newest is shown */
            seen++;
            last_msg = Clock::now();
            rate.hit(last_msg);
            latest.data.assign(m->data().data(), m->data().data() + m->data().size());
            latest.schema = rant::Schema(m->raw_schema());
            latest.from = std::string(m->publisher_name());
            latest.any = dirty = true;
            if (count && seen >= count) break;
            m = sub.take(std::chrono::milliseconds(0));
        }
        auto now = Clock::now();
        /* a new value redraws at 30 frames a second at most, and the rate and age keep
           moving between messages at 4 */
        bool done = count && seen >= count;
        bool due = (dirty && now - last_draw >= std::chrono::milliseconds(33)) ||
                   now - last_draw >= std::chrono::milliseconds(250);
        if (!due && !done) continue;

        std::string status;
        if (!latest.any) {
            status = "waiting for a message";
        } else {
            double age = std::chrono::duration<double>(now - last_msg).count();
            std::string hz = fixed(rate.hz(now), 1);
            hz.insert(0, hz.size() < 6 ? 6 - hz.size() : 0, ' ');    /* the header holds still too */
            status = "from " + latest.from + "  " + hz + " Hz  " + std::to_string(seen) +
                     (seen == 1 ? " message" : " messages");
            if (age > 1) status += "  last " + fixed(age, 1) + " s ago";
        }
        std::vector<std::string> lines{ ctx.out.paint(ui::Style::Bold, topic) + "  " + ctx.out.paint(ui::Style::Faint, status) };
        if (latest.any) {
            std::string body = yaml.block(mesh::to_json(rant::Bytes(latest.data.data(), latest.data.size()), latest.schema));
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

/* One record per message: a YAML line, a JSON line or a CSV row. */
static void stream(app::Context& ctx, rant::Subscriber<rant::Bytes>& sub, long count, Format format) {
    ui::Yaml yaml(&ctx.out, true);
    ui::Csv csv;
    long seen = 0;
    while (!util::interrupted() && (count == 0 || seen < count)) {
        auto m = sub.take(std::chrono::milliseconds(100));
        if (!m) continue;
        json value = mesh::to_json(m->data(), rant::Schema(m->raw_schema()));
        if (format == Format::Json)
            ctx.out.line(json{ { "from", std::string(m->publisher_name()) }, { "value", value } }.dump(
                -1, ' ', false, json::error_handler_t::replace));
        else if (format == Format::Csv)
            ctx.out.line(csv.rows(value));
        else
            ctx.out.line(yaml.flow(value));
        std::fflush(stdout);
        seen++;
    }
}

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("sub takes one topic");
    const std::string& topic = ctx.args.words[0];
    long count = 0;
    if (auto c = ctx.args.get("count")) {
        count = std::strtol(c->c_str(), nullptr, 10);
        if (count <= 0) throw app::UsageError("--count takes a positive number");
    }
    Format format = format_of(ctx);
    bool live_view = format == Format::Yaml && !ctx.args.has("lines") && ui::enable_vt(stdout);

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    if (!mesh.node().reflection().find(rant::EntityKind::Topic, topic))
        ctx.out.note("no publisher of `" + topic + "` yet, waiting for one");

    rant::Qos qos;
    qos.reflect_from_mesh = true;
    auto sub = mesh.node().subscriber<rant::Bytes>(topic, qos);
    if (live_view) live(ctx, sub, topic, count);
    else stream(ctx, sub, count, format);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Topic }), true };
}

app::Command sub() {
    app::Command c{ "sub", "<topic>", "show the newest message on a topic until Ctrl-C", app::Section::Mesh,
                    { { "count", 'n', "N", "stop after N messages" },
               { "lines", 'l', "", "print each message on its own line instead of updating in place" },
               csv_option() },
                    run };
    c.complete = complete_words;
    return c;
}

}
