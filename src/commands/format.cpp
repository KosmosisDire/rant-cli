#include "commands/format.hpp"

#include "app/failure.hpp"
#include "ui/terminal.hpp"

namespace commands {

using json = nlohmann::ordered_json;

app::OptionSpec csv_option() { return { "csv", 0, "", "print CSV, one column per field" }; }

app::OptionSpec lines_option() {
    return { "lines", 'l', "", "print each record on its own line instead of updating in place" };
}

Format format_of(const app::Context& ctx) {
    bool csv = ctx.args.has("csv");
    if (csv && ctx.json) throw app::UsageError("give --json or --csv, not both");
    return csv ? Format::Csv : ctx.json ? Format::Json : Format::Yaml;
}

bool live_of(const app::Context& ctx) { return !ctx.args.has("lines") && ui::enable_vt(stdout); }

Printer::Printer(Format format, const ui::Output* out, bool stable) : format_(format), yaml_(out, stable) {}

std::string Printer::block(const json& v) {
    switch (format_) {
    case Format::Json: return v.dump(2, ' ', false, json::error_handler_t::replace);
    case Format::Csv:  return ui::Csv().rows(v);
    case Format::Yaml: break;
    }
    return yaml_.block(v);
}

std::string Printer::line(const json& v) {
    switch (format_) {
    case Format::Json: return v.dump(-1, ' ', false, json::error_handler_t::replace);
    case Format::Csv:  return csv_.rows(v);
    case Format::Yaml: break;
    }
    return yaml_.flow(v);
}

void print_value(app::Context& ctx, const json& v) {
    ctx.out.line(Printer(format_of(ctx), &ctx.out, false).block(v));
}

}
