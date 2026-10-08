#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "commands/format.hpp"
#include "mesh/access.hpp"

namespace commands {

static int run(app::Context& ctx) {
    if (ctx.args.words.size() != 1) throw app::UsageError("get takes one variable");
    const std::string& name = ctx.args.words[0];
    format_of(ctx);
    mesh::Client mesh(ctx.domain);
    mesh.settle();
    mesh::require_entity(mesh, { rant::EntityKind::Variable }, name);
    auto value = mesh::read_variable(mesh, name);
    if (!value) throw app::Failure("the owner of `" + name + "` sent no value in time");
    print_value(ctx, *value);
    return 0;
}

app::Command get() {
    return { "get", "<variable>", "print a variable's current value", app::Section::Mesh, { csv_option() }, run };
}

}
