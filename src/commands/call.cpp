#include <atomic>
#include <chrono>
#include <cstdlib>
#include <thread>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/access.hpp"
#include "util/interrupt.hpp"

namespace commands {

using mesh::json;

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
[[noreturn]] static void fail(const std::string& name, const rant::Response<rant::Bytes>& r) {
    if (r.send_status() != rant::SendStatus::Ok)
        throw app::Failure("cannot call `" + name + "`: " + mesh::send_failure(r.send_status()));
    std::string why = status_text(r.status());
    if (!r.message().empty() && r.message() != why) why += ": " + std::string(r.message());
    throw app::Failure("calling `" + name + "`: " + why);
}

static void print(app::Context& ctx, const char* key, const json& v) {
    if (ctx.json) ctx.out.line(json{ { key, v } }.dump(-1, ' ', false, json::error_handler_t::replace));
    else ctx.out.line(v.dump(-1, ' ', false, json::error_handler_t::replace));
    std::fflush(stdout);
}

static int call_function(app::Context& ctx, mesh::Client& mesh, const std::string& name,
                         const mesh::Assignments& values, std::chrono::milliseconds timeout) {
    rant::FunctionOptions o;
    o.reflect_from_mesh = true;
    auto fn = mesh.node().remote_function<rant::Bytes, rant::Bytes>(name, o);
    std::vector<uint8_t> req = mesh::encode(values, fn.request_schema());
    auto r = fn.call(rant::Bytes(req.data(), req.size()), timeout);
    if (!r) fail(name, r);
    print(ctx, "result", mesh::to_json(r.data(), rant::Schema(r.raw_schema())));
    return 0;
}

/* A task prints each progress update as it comes and the result last. Ctrl-C asks the
 * provider to cancel, and the call ends with the outcome it answers. */
static int call_task(app::Context& ctx, mesh::Client& mesh, const std::string& name,
                     const mesh::Assignments& values, std::chrono::milliseconds timeout) {
    rant::TaskOptions o;
    o.reflect_from_mesh = true;
    auto task = mesh.node().remote_task<rant::Bytes, rant::Bytes, rant::Bytes>(name, o);
    std::vector<uint8_t> req = mesh::encode(values, task.request_schema());

    std::atomic<uint32_t> id{ 0 };
    std::atomic<bool> done{ false };
    std::thread watcher([&] {
        bool asked = false;
        while (!done) {
            if (!asked && util::interrupted() && id) {
                asked = true;
                ctx.out.note("cancelling task " + std::to_string(id.load()));
                rant::SendStatus st = task.cancel(id);
                if (st != rant::SendStatus::Ok) ctx.out.warn("the provider cannot cancel it: " + mesh::send_failure(st));
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
    });
    auto progress = [&](const rant::ProgressView<rant::Bytes>& p) {
        if (!p.has_value()) {    /* the provider's start acknowledgment */
            id = p.call_id();
            ctx.out.note("task " + std::to_string(p.call_id()) + " running, Ctrl-C cancels it");
            return;
        }
        id = p.call_id();
        print(ctx, "progress", mesh::to_json(p.data(), rant::Schema(p.raw_schema())));
    };
    auto r = task.call(rant::Bytes(req.data(), req.size()), progress, timeout);
    done = true;
    watcher.join();
    if (r.status() == rant::CallStatus::Cancelled && r.data().size())
        print(ctx, "partial", mesh::to_json(r.data(), rant::Schema(r.raw_schema())));
    if (!r) fail(name, r);
    print(ctx, "result", mesh::to_json(r.data(), rant::Schema(r.raw_schema())));
    return 0;
}

static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.empty()) throw app::UsageError("call takes a function or task and its arguments");
    const std::string& name = w[0];
    mesh::Assignments values;
    if (w.size() > 1) values = mesh::parse_words({ w.begin() + 1, w.end() });
    std::chrono::milliseconds timeout(5000);
    if (auto t = ctx.args.get("timeout")) {
        double s = std::atof(t->c_str());
        if (s <= 0) throw app::UsageError("--timeout takes seconds, more than 0");
        timeout = std::chrono::milliseconds((long long)(s * 1000));
    }

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    rant::Entity e = mesh::require_entity(mesh, { rant::EntityKind::Function, rant::EntityKind::Task }, name);
    if (e.kind == rant::EntityKind::Task) return call_task(ctx, mesh, name, values, timeout);
    return call_function(ctx, mesh, name, values, timeout);
}

app::Command call() {
    return { "call", "<function|task> [value]", "call a function and print the reply, or run a task", app::Section::Mesh,
             { { "timeout", 't', "S", "seconds to wait for an answer, 5 by default" } }, run };
}

}
