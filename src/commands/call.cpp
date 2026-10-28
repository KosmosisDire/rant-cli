#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <memory>
#include <mutex>
#include <optional>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "commands/stream.hpp"
#include "complete/complete.hpp"
#include "mesh/access.hpp"
#include "util/interrupt.hpp"

namespace commands {

static std::string status_text(rant::CallStatus s) {
    switch (s) {
    case rant::CallStatus::Ok:         return "ok";
    case rant::CallStatus::AppError:   return "failed";
    case rant::CallStatus::NoHandler:  return "the definition has no handler";
    case rant::CallStatus::Timeout:    return "no answer in time";
    case rant::CallStatus::PeerLost:   return "the provider went away";
    case rant::CallStatus::Cancelled:  return "cancelled";
    case rant::CallStatus::Running:    return "it is still running";
    case rant::CallStatus::NoProvider: return "nobody provides it";
    }
    return "it failed";
}

/* The outcome of a call that did not succeed, with the provider's own words when it gave
 * any. */
[[noreturn]] static void fail(const std::string& name, rant::CallStatus status, std::string_view message) {
    std::string why = status_text(status);
    if (!message.empty() && message != why) why += ": " + std::string(message);
    throw app::Failure("calling `" + name + "`: " + why);
}

static int call_function(app::Context& ctx, mesh::Client& mesh, const std::string& name,
                         const mesh::Assignments& values, std::chrono::milliseconds timeout) {
    rant::FunctionOptions o;
    o.reflect_from_mesh = true;
    auto fn = mesh.node().remote_function<rant::Bytes, rant::Bytes>(name, o);
    std::vector<uint8_t> req = mesh::encode(values, fn.request_schema());
    auto r = fn.call(rant::Bytes(req.data(), req.size()), timeout);
    if (r.send_status() != rant::SendStatus::Ok)
        throw app::Failure("cannot call `" + name + "`: " + mesh::send_failure(r.send_status()));
    if (!r) fail(name, r.status(), r.message());
    print_value(ctx, mesh::to_json(r.data(), rant::Schema(r.raw_schema())));
    return 0;
}

/* A task's outcome, copied off the thread that answers. */
struct Outcome {
    rant::CallStatus     status;
    std::string          message;
    std::vector<uint8_t> data;
    rant::Schema         schema;

    mesh::json value() const { return mesh::to_json(rant::Bytes(data.data(), data.size()), schema); }
};

/* Shared with the call's handlers, which may still fire after the caller left. */
struct TaskState {
    std::mutex              mu;
    std::condition_variable cv;
    std::optional<Outcome>  outcome;
    bool                    left = false;    /* the caller stopped waiting, the handlers touch nothing of it */
};

/* How long a cancelled task may take to answer before the caller leaves without it. */
static constexpr std::chrono::seconds cancel_wait{ 10 };

/* A task streams its progress as sub does and prints the result last. Ctrl-C asks the
 * provider to cancel and waits for its answer, at most cancel_wait. */
static int call_task(app::Context& ctx, mesh::Client& mesh, const rant::Entity& e,
                     const mesh::Assignments& values, std::chrono::milliseconds timeout) {
    const std::string& name = e.name;
    rant::TaskOptions o;
    o.reflect_from_mesh = true;
    o.timeout = timeout;
    auto task = mesh.node().remote_task<rant::Bytes, rant::Bytes, rant::Bytes>(name, o);
    std::vector<uint8_t> req = mesh::encode(values, task.request_schema());

    ValueStream out(ctx, name);
    std::string from = mesh.provider_of(e);
    /* CSV lines leave progress out, so the output stays one table: the result's */
    bool show_progress = live_of(ctx) || format_of(ctx) != Format::Csv;
    auto st = std::make_shared<TaskState>();
    auto progress = [st, &ctx, &out, from, show_progress](const rant::ProgressView<rant::Bytes>& p) {
        std::lock_guard<std::mutex> g(st->mu);
        if (st->left) return;
        if (!p.has_value())    /* the provider's start acknowledgment */
            ctx.out.note("task " + std::to_string(p.call_id()) + " running, Ctrl-C cancels it");
        else if (show_progress)
            out.add(from, p.data(), rant::Schema(p.raw_schema()));
    };
    auto answer = [st](const rant::ResponseView<rant::Bytes>& r) {
        std::lock_guard<std::mutex> g(st->mu);
        st->outcome = Outcome{ r.status(), std::string(r.message()), { r.data().data(), r.data().data() + r.data().size() },
                               rant::Schema(r.raw_schema()) };
        st->cv.notify_one();
    };
    rant::TaskCall call = task.call_async(rant::Bytes(req.data(), req.size()), progress, answer);
    if (!call.ok()) throw app::Failure("cannot call `" + name + "`: " + mesh::send_failure(call.status));

    std::unique_lock<std::mutex> g(st->mu);
    std::optional<std::chrono::steady_clock::time_point> cancelled;
    while (!st->outcome) {
        st->cv.wait_for(g, std::chrono::milliseconds(50));
        if (st->outcome) break;
        if (!cancelled && util::interrupted()) {
            cancelled = std::chrono::steady_clock::now();
            out.note("cancelling task " + std::to_string(call.id) + ", waiting up to " +
                     std::to_string(cancel_wait.count()) + " s for " + from + " to stop it");
            g.unlock();    /* the handlers take the lock, and a cancel may wait on them */
            rant::SendStatus s = task.cancel(call.id);
            g.lock();
            if (s != rant::SendStatus::Ok) {
                st->left = true;
                out.finish();
                throw app::Failure("cannot cancel task " + std::to_string(call.id) + ": " + mesh::send_failure(s) +
                                   ", it may still run at " + from);
            }
        }
        if (cancelled && std::chrono::steady_clock::now() - *cancelled >= cancel_wait) {
            st->left = true;
            out.finish();
            throw app::Failure("task " + std::to_string(call.id) + " did not stop within " +
                               std::to_string(cancel_wait.count()) + " s, it may still run at " + from);
        }
    }
    st->left = true;
    Outcome r = std::move(*st->outcome);
    g.unlock();

    bool partial = r.status == rant::CallStatus::Cancelled && !r.data.empty();
    bool ok = r.status == rant::CallStatus::Ok;
    out.finish(partial ? "partial result" : ok ? "result" : "");
    if (partial) print_value(ctx, r.value());
    if (!ok) fail(name, r.status, r.message);
    print_value(ctx, r.value());
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) throw app::UsageError("say which function or task to call");
    const std::string& name = w[0];
    mesh::Assignments values;
    if (w.size() > 1) values = mesh::parse_words({ w.begin() + 1, w.end() });
    std::chrono::milliseconds timeout(5000);
    if (auto t = ctx.args.get("timeout")) {
        double s = std::atof(t->c_str());
        if (s <= 0) throw app::UsageError("--timeout takes seconds, more than 0");
        timeout = std::chrono::milliseconds((long long)(s * 1000));
    }

    format_of(ctx);
    util::catch_interrupt();
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    rant::Entity e = mesh::require_entity(mesh, { rant::EntityKind::Function, rant::EntityKind::Task }, name);
    if (e.kind == rant::EntityKind::Task) return call_task(ctx, mesh, e, values, timeout);
    return call_function(ctx, mesh, name, values, timeout);
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Function, rant::EntityKind::Task }), true };
}

app::Command call() {
    app::Command c{ "call", "<function|task> [value]", "call a function and print the reply, or run a task", app::Section::Mesh,
                    { { "timeout", 't', "S", "seconds to wait for an answer, 5 by default" }, lines_option(), value_only_option(),
                      csv_option() },
                    run };
    c.complete = complete_words;
    return c;
}

}
