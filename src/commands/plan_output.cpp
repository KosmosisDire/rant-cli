#include "commands/plan_output.hpp"

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "process/command.hpp"

namespace commands {

using json = nlohmann::ordered_json;

void require_plan(app::Context& ctx, const config::Plan& plan) {
    if (plan.diagnostics.empty()) return;
    for (auto& d : plan.diagnostics) ctx.out.error(d.str());
    throw app::Failure("");
}

void print_plan(app::Context& ctx, const config::Plan& plan) {
    if (ctx.json) {
        json nodes = json::array();
        for (auto& i : plan.instances)
            nodes.push_back({ { "name", i.name },
                              { "type", i.type },
                              { "argv", i.argv },
                              { "env", i.env },
                              { "cwd", config::to_utf8(i.cwd) } });
        ctx.out.line(json{ { "workspace", config::to_utf8(plan.workspace->root) }, { "nodes", nodes } }.dump(2));
        return;
    }
    for (auto& i : plan.instances) {
        ctx.out.line(ctx.out.paint(ui::Style::Bold, i.name) + "  " + ctx.out.paint(ui::Style::Dim, i.type));
        ctx.out.line("  run  " + process::shown(i.argv));
        ctx.out.line("  cwd  " + config::to_utf8(i.cwd));
        for (auto& [k, v] : i.env) ctx.out.line("  env  " + k + "=" + v);
    }
}

}
