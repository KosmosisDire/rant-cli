#include "commands/plan_output.hpp"

#include <nlohmann/json.hpp>

#include "app/failure.hpp"
#include "process/command.hpp"
#include "ui/table.hpp"

namespace commands {

using json = nlohmann::ordered_json;

void require_plan(app::Context& ctx, const config::Plan& plan) {
    if (plan.diagnostics.empty()) return;
    for (auto& d : plan.diagnostics) ctx.out.error(d.str());
    throw app::Failure("");
}

void print_params(app::Context& ctx, const std::vector<config::Param>& params) {
    if (params.empty()) return;
    ui::Table t;
    for (auto& p : params) {
        std::string about = p.default_value ? "default " + *p.default_value : "required";
        if (!p.options.empty()) {
            about += ", one of";
            for (size_t i = 0; i < p.options.size(); i++) about += (i ? ", " : " ") + p.options[i];
        }
        t.row({ p.name + "=<" + p.type + ">", about, p.description });
    }
    ctx.out.line("\nParams:");
    for (auto& l : t.lines()) ctx.out.line(l);
}

void print_plan(app::Context& ctx, const config::Plan& plan) {
    if (ctx.json) {
        json nodes = json::array();
        for (auto& i : plan.instances)
            nodes.push_back({ { "name", i.name },
                              { "node", i.type },
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
