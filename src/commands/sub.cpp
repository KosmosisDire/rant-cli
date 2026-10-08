#include <cstdlib>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "mesh/client.hpp"
#include "mesh/values.hpp"
#include "util/interrupt.hpp"

namespace commands {

using mesh::json;

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("sub takes one topic");
    const std::string& topic = ctx.args.words[0];
    long count = 0;
    if (auto c = ctx.args.get("count")) {
        count = std::strtol(c->c_str(), nullptr, 10);
        if (count <= 0) throw app::UsageError("--count takes a positive number");
    }

    util::catch_interrupt();
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    if (!mesh.node().reflection().find(rant::EntityKind::Topic, topic))
        ctx.out.note("no publisher of `" + topic + "` yet, waiting for one");

    rant::Qos qos;
    qos.reflect_from_mesh = true;
    auto sub = mesh.node().subscriber<rant::Bytes>(topic, qos);

    long seen = 0;
    while (!util::interrupted() && (count == 0 || seen < count)) {
        auto m = sub.take(std::chrono::milliseconds(100));
        if (!m) continue;
        rant::Schema schema(m->raw_schema());
        if (ctx.json) {
            json line = { { "from", std::string(m->publisher_name()) }, { "value", mesh::to_json(m->data(), schema) } };
            ctx.out.line(line.dump(-1, ' ', false, json::error_handler_t::replace));
        } else {
            ctx.out.line(mesh::to_text(m->data(), schema));
        }
        std::fflush(stdout);
        seen++;
    }
    return 0;
}

app::Command sub() {
    return { "sub", "<topic>", "print each message on a topic until Ctrl-C", app::Section::Mesh,
             { { "count", 'n', "N", "stop after N messages" } }, run };
}

}
