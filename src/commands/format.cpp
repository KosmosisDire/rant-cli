#include "commands/format.hpp"

#include "app/failure.hpp"
#include "ui/csv.hpp"
#include "ui/yaml.hpp"

namespace commands {

app::OptionSpec csv_option() { return { "csv", 0, "", "print CSV, one column per field" }; }

Format format_of(const app::Context& ctx) {
    bool csv = ctx.args.has("csv");
    if (csv && ctx.json) throw app::UsageError("give --json or --csv, not both");
    return csv ? Format::Csv : ctx.json ? Format::Json : Format::Yaml;
}

void print_value(app::Context& ctx, const nlohmann::ordered_json& v) {
    switch (format_of(ctx)) {
    case Format::Json:
        ctx.out.line(v.dump(2, ' ', false, nlohmann::ordered_json::error_handler_t::replace));
        break;
    case Format::Csv: {
        ui::Csv csv;
        ctx.out.line(csv.rows(v));
        break;
    }
    case Format::Yaml: {
        ui::Yaml yaml(&ctx.out, false);
        ctx.out.line(yaml.block(v));
        break;
    }
    }
}

}
