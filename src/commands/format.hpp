#pragma once

#include <nlohmann/json.hpp>

#include "app/args.hpp"
#include "app/context.hpp"
#include "ui/csv.hpp"
#include "ui/yaml.hpp"

/* How commands print mesh values. The format is how a value is written, the display is
 * whether a stream redraws in place or prints a line per record. */
namespace commands {

enum class Format { Yaml, Json, Csv };

/* The --csv option, for the commands that print values. */
app::OptionSpec csv_option();

/* The --lines and --value-only options, for the commands that print a stream. */
app::OptionSpec lines_option();
app::OptionSpec value_only_option();

/* The format asked for: YAML, JSON with the global --json, CSV with --csv, not both. */
Format format_of(const app::Context& ctx);

/* Whether a stream redraws in place: only on a terminal, and not with --lines. */
bool live_of(const app::Context& ctx);

/* Values written in one format. out paints YAML, nullptr prints it plain. stable keeps YAML
 * numbers in place from one value to the next. */
class Printer {
public:
    Printer(Format format, const ui::Output* out, bool stable);

    /* A whole value: a YAML block, indented JSON, or a CSV header and row. No newline at the end. */
    std::string block(const nlohmann::ordered_json& v);

    /* One record of a stream: a YAML flow line, a JSON line, or a CSV row after a header the first time. */
    std::string line(const nlohmann::ordered_json& v);

private:
    Format   format_;
    ui::Yaml yaml_;
    ui::Csv  csv_;
};

/* One value in the format asked for, such as a call's result: whole on a terminal, one line
 * with --lines or in a pipe. */
void print_value(app::Context& ctx, const nlohmann::ordered_json& v);

}
