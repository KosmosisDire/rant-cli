#include <chrono>
#include <thread>

#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "mesh/access.hpp"

namespace commands {

using namespace std::chrono_literals;

/* Writes a variable, then waits to see the owner apply it and prints what it holds now. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (w.size() < 2) throw app::UsageError("set takes a variable and a value, such as `rant set speed 2.5`");
    const std::string& name = w[0];
    mesh::Assignments values = mesh::parse_words({ w.begin() + 1, w.end() });
    format_of(ctx);

    mesh::Client mesh(ctx.domain);
    mesh.settle();
    rant::Entity e = mesh::require_entity(mesh, { rant::EntityKind::Variable }, name);
    if (!e.writable) throw app::Failure("`" + name + "` is read only");

    rant::VariableOptions<rant::Bytes> o;
    o.reflect_from_mesh = true;
    auto var = mesh.node().remote_variable<rant::Bytes>(name, o);
    if (!var.wait(2s)) throw app::Failure("the owner of `" + name + "` did not answer in time");
    std::vector<uint8_t> bytes = mesh::encode(values, var.schema());
    rant::SendStatus st = var.set(rant::Bytes(bytes.data(), bytes.size()));
    if (st != rant::SendStatus::Ok) throw app::Failure("cannot set `" + name + "`: " + mesh::send_failure(st));

    auto deadline = std::chrono::steady_clock::now() + 2s;
    std::optional<std::vector<uint8_t>> now;
    while (std::chrono::steady_clock::now() < deadline) {
        now = var.get();
        if (now && *now == bytes) break;
        std::this_thread::sleep_for(10ms);
    }
    if (!now || *now != bytes) throw app::Failure("the owner of `" + name + "` did not apply the value in time");
    print_value(ctx, mesh::to_json(rant::Bytes(now->data(), now->size()), var.schema()));
    return 0;
}

app::Command set() {
    return { "set", "<variable> <value>", "write a variable and print what it holds after", app::Section::Mesh,
             { csv_option() }, run };
}

}
