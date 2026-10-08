#pragma once

#include <nlohmann/json.hpp>

#include "app/args.hpp"
#include "app/context.hpp"

/* How a command that prints values prints them: YAML for a person by default, JSON with
 * the global --json, CSV with --csv. */
namespace commands {

enum class Format { Yaml, Json, Csv };

/* The --csv option, for the commands that print values. */
app::OptionSpec csv_option();

/* The format asked for. Both --json and --csv at once is a usage error. */
Format format_of(const app::Context& ctx);

/* One value in the format asked for, the end of a get, a set or a call. */
void print_value(app::Context& ctx, const nlohmann::ordered_json& v);

}
