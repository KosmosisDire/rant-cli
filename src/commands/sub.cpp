#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "commands/stream.hpp"
#include "complete/complete.hpp"
#include "mesh/client.hpp"
#include "util/interrupt.hpp"

namespace commands {

/* Adds the next value of a topic or a variable to the stream, false when none came within wait. */
using Source = std::function<bool(ValueStream& out, std::chrono::milliseconds wait)>;

static Source topic_source(rant::Subscriber<rant::Bytes>& sub) {
    return [&sub](ValueStream& out, std::chrono::milliseconds wait) {
        auto m = sub.take(wait);
        if (!m) return false;
        out.add(std::string(m->publisher_name()), m->data(), rant::Schema(m->raw_schema()));
        return true;
    };
}

/* A variable's changes, the current value first. The queue is shared with the handler, which
 * lives until the node closes. */
static Source variable_source(rant::RemoteVariable<rant::Bytes>& var, const std::string& owner) {
    struct Change {
        std::vector<uint8_t> data;
        rant::Schema         schema;
    };
    struct Queue {
        std::mutex              mu;
        std::condition_variable cv;
        std::deque<Change>      changes;
    };
    auto q = std::make_shared<Queue>();
    var.on_change([q](const rant::VariableUpdate& u) {
        std::lock_guard<std::mutex> g(q->mu);
        q->changes.push_back({ { u.value().data(), u.value().data() + u.value().size() }, rant::Schema(u.raw_schema()) });
        q->cv.notify_one();
    });
    return [q, owner](ValueStream& out, std::chrono::milliseconds wait) {
        std::unique_lock<std::mutex> g(q->mu);
        if (!q->cv.wait_for(g, wait, [&] { return !q->changes.empty(); })) return false;
        Change c = std::move(q->changes.front());
        q->changes.pop_front();
        g.unlock();
        out.add(owner, rant::Bytes(c.data.data(), c.data.size()), c.schema);
        return true;
    };
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
        next = variable_source(*var, mesh.provider_of(*e));
    } else {
        if (!mesh.node().reflection().find(rant::EntityKind::Topic, name))
            ctx.out.note("no publisher of `" + name + "` yet, waiting for one");
        rant::Qos qos;
        qos.reflect_from_mesh = true;
        sub.emplace(mesh.node().subscriber<rant::Bytes>(name, qos));
        next = topic_source(*sub);
    }

    ValueStream out(ctx, name);
    auto more = [&] { return count == 0 || out.seen() < count; };
    while (!util::interrupted() && more()) {
        bool got = next(out, std::chrono::milliseconds(50));
        while (got && more()) got = next(out, std::chrono::milliseconds(0));    /* every waiting value counts */
        out.tick();
    }
    out.finish();
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Topic, rant::EntityKind::Variable }), true };
}

app::Command sub() {
    app::Command c{ "sub", "<topic|variable>", "show the newest value of a topic or variable until Ctrl-C", app::Section::Mesh,
                    { { "count", 'n', "N", "stop after N values" }, lines_option(), value_only_option(), csv_option() },
                    run };
    c.complete = complete_words;
    return c;
}

}
