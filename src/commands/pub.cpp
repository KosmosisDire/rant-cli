#include <chrono>
#include <cstdlib>
#include <thread>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "mesh/access.hpp"
#include "util/interrupt.hpp"

namespace commands {

/* Publishes one value, or the same value at a rate until --count or Ctrl-C. The type is
 * the one every subscriber of the topic accepts. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() < 2) throw app::UsageError("pub takes a topic and a value, such as `rant pub chatter x=1`");
    const std::string& topic = w[0];
    mesh::Assignments values = mesh::parse_words({ w.begin() + 1, w.end() });
    double rate = 0;
    long count = 1;
    if (auto r = ctx.args.get("rate")) {
        rate = std::atof(r->c_str());
        if (rate <= 0) throw app::UsageError("--rate takes messages a second, more than 0");
        count = 0;
    }
    if (auto c = ctx.args.get("count")) {
        count = std::strtol(c->c_str(), nullptr, 10);
        if (count <= 0) throw app::UsageError("--count takes a positive number");
    }

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain());
    mesh.settle();
    mesh::require_entity(mesh, { rant::EntityKind::Topic }, topic);

    rant::Qos qos;
    qos.reflect_from_mesh = true;
    auto pub = mesh.node().publisher<rant::Bytes>(topic, qos);
    std::vector<uint8_t> bytes = mesh::encode(values, pub.schema());

    auto period = rate > 0 ? std::chrono::duration<double>(1.0 / rate) : std::chrono::duration<double>(0);
    auto next = std::chrono::steady_clock::now();
    long sent = 0;
    while (!util::interrupted() && (count == 0 || sent < count)) {
        rant::SendStatus st = pub.send(rant::Bytes(bytes.data(), bytes.size()));
        if (st != rant::SendStatus::Ok) throw app::Failure("cannot publish to `" + topic + "`: " + mesh::send_failure(st));
        sent++;
        if (count != 0 && sent >= count) break;
        next += std::chrono::duration_cast<std::chrono::steady_clock::duration>(period);
        std::this_thread::sleep_until(next);
    }
    pub.drain(std::chrono::seconds(1));
    ctx.out.note("published " + std::to_string(sent) + (sent == 1 ? " message" : " messages") + " to " + topic);
    return 0;
}

static complete::Candidates complete_words(complete::Request& r) {
    if (!r.words.empty()) return {};
    return { r.entities({ rant::EntityKind::Topic }), true };
}

app::Command pub() {
    app::Command c{ "pub", "<topic> <value>", "publish a value once, or at a rate", app::Section::Mesh,
                    { { "rate", 'r', "HZ", "publish this many times a second until Ctrl-C" },
               { "count", 'n', "N", "stop after N messages" } },
                    run };
    c.complete = complete_words;
    return c;
}

}
