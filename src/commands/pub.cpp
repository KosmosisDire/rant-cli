#include <chrono>
#include <cstdlib>
#include <functional>
#include <thread>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "mesh/access.hpp"
#include "util/interrupt.hpp"

namespace commands {

using namespace std::chrono_literals;

/* Sends one value, or the same value at a rate until --count or Ctrl-C. Returns how many went.
 * what names the act in a failure, such as "publish to `chatter`". */
static long send_all(const std::string& what, double rate, long count, const std::function<rant::SendStatus()>& send) {
    auto period = rate > 0 ? std::chrono::duration<double>(1.0 / rate) : std::chrono::duration<double>(0);
    auto next = std::chrono::steady_clock::now();
    long sent = 0;
    while (!util::interrupted() && (count == 0 || sent < count)) {
        rant::SendStatus st = send();
        if (st != rant::SendStatus::Ok) throw app::Failure("cannot " + what + ": " + mesh::send_failure(st));
        sent++;
        if (count != 0 && sent >= count) break;
        next += std::chrono::duration_cast<std::chrono::steady_clock::duration>(period);
        std::this_thread::sleep_until(next);
    }
    return sent;
}

static std::string times(long n, const char* one, const char* many) {
    return std::to_string(n) + " " + (n == 1 ? one : many);
}

/* A topic takes the type every subscriber accepts. A variable takes its owner's type, and the
 * write counts once the owner holds the value. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() < 2) throw app::UsageError("pub takes a topic or variable and a value, such as `rant pub speed 2.5`");
    const std::string& name = w[0];
    mesh::Assignments values = mesh::parse_words({ w.begin() + 1, w.end() });
    double rate = 0;
    long count = 1;
    if (auto r = ctx.args.get("rate")) {
        rate = std::atof(r->c_str());
        if (rate <= 0) throw app::UsageError("--rate takes values a second, more than 0");
        count = 0;
    }
    if (auto c = ctx.args.get("count")) {
        count = std::strtol(c->c_str(), nullptr, 10);
        if (count <= 0) throw app::UsageError("--count takes a positive number");
    }

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    rant::Entity e = mesh::require_entity(mesh, { rant::EntityKind::Topic, rant::EntityKind::Variable }, name);

    if (e.kind == rant::EntityKind::Topic) {
        rant::Qos qos;
        qos.reflect_from_mesh = true;
        auto pub = mesh.node().publisher<rant::Bytes>(name, qos);
        std::vector<uint8_t> bytes = mesh::encode(values, pub.schema());
        long sent = send_all("publish to `" + name + "`", rate, count,
                             [&] { return pub.send(rant::Bytes(bytes.data(), bytes.size())); });
        pub.drain(1s);
        ctx.out.note("published " + times(sent, "message", "messages") + " to " + name);
        return 0;
    }

    if (!e.writable) throw app::Failure("`" + name + "` is read only");
    rant::VariableOptions<rant::Bytes> o;
    o.reflect_from_mesh = true;
    auto var = mesh.node().remote_variable<rant::Bytes>(name, o);
    if (!var.wait(2s)) throw app::Failure("the owner of `" + name + "` did not answer in time");
    std::vector<uint8_t> bytes = mesh::encode(values, var.schema());
    long sent = send_all("write `" + name + "`", rate, count, [&] { return var.set(rant::Bytes(bytes.data(), bytes.size())); });

    auto deadline = std::chrono::steady_clock::now() + 2s;
    std::optional<std::vector<uint8_t>> now;
    while (std::chrono::steady_clock::now() < deadline) {
        now = var.get();
        if (now && *now == bytes) break;
        std::this_thread::sleep_for(10ms);
    }
    if (!now || *now != bytes) throw app::Failure("the owner of `" + name + "` did not apply the value in time");
    ctx.out.note("wrote " + times(sent, "value", "values") + " to " + name);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Topic, rant::EntityKind::Variable }), true };
}

app::Command pub() {
    app::Command c{ "pub", "<topic|variable> <value>", "publish to a topic or write a variable, once or at a rate",
                    app::Section::Mesh,
                    { { "rate", 'r', "HZ", "send this many times a second until Ctrl-C" },
               { "count", 'n', "N", "stop after N values" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
