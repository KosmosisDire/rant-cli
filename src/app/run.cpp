#include <cstdlib>

#include "app/command.hpp"
#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "rant.hpp"

namespace app {

const std::vector<OptionSpec>& global_options() {
    static const std::vector<OptionSpec> opts = {
        { "yes",     'y', "",  "answer yes to every question" },
        { "json",    0,   "",  "print JSON instead of text" },
        { "domain",  0,   "N", "the Rant domain to use, 0 by default" },
        { "help",    'h', "",  "show help, also after a command" },
        { "version", 0,   "",  "show the version" },
    };
    return opts;
}

static std::string option_lines(const std::vector<OptionSpec>& specs) {
    std::string s;
    for (auto& o : specs) {
        std::string left = o.short_name ? std::string("-") + o.short_name + ", " : "    ";
        left += "--" + std::string(o.name);
        if (!o.value_name.empty()) left += " " + std::string(o.value_name);
        if (left.size() < 22) left.resize(22, ' ');
        s += "  " + left + " " + std::string(o.help) + "\n";
    }
    return s;
}

std::string command_help(const Command& c) {
    std::string s = "Usage: rant " + std::string(c.name);
    if (!c.usage.empty()) s += " " + std::string(c.usage);
    s += "\n\n" + std::string(c.summary) + "\n";
    if (!c.options.empty()) s += "\nOptions:\n" + option_lines(c.options);
    return s;
}

static std::string main_help() {
    std::string s =
        "rant: build, run and inspect Rant nodes\n\n"
        "Usage: rant <command> [args] [options]\n";
    for (auto section : { Section::Workspace, Section::Mesh, Section::Setup }) {
        s += section == Section::Workspace ? "\nIn a workspace:\n"
             : section == Section::Mesh    ? "\nOn the mesh, anywhere:\n"
                                           : "\nSetting up:\n";
        for (auto& c : commands::all()) {
            if (c.section != section) continue;
            std::string left = std::string(c.name) + " " + std::string(c.usage);
            if (left.size() > 30) left += "\n" + std::string(33, ' ');    /* a long usage takes its own line */
            else left.resize(31, ' ');
            s += "  " + left + std::string(c.summary) + "\n";
        }
    }
    s += "\nOptions:\n" + option_lines(global_options());
    s += "\nRun `rant <command> --help` for the details of one command.\n";
    return s;
}

static const Command* find(std::string_view name) {
    for (auto& c : commands::all())
        if (c.name == name) return &c;
    return nullptr;
}

static uint16_t parse_domain(const std::string& s) {
    char* end = nullptr;
    unsigned long v = std::strtoul(s.c_str(), &end, 10);
    if (s.empty() || *end || v > 65535) throw UsageError("--domain takes a number from 0 to 65535");
    return (uint16_t)v;
}

int run(Context& ctx, const std::vector<std::string>& tokens) {
    if (!tokens.empty() && tokens[0] == "__complete") return complete::run(ctx, { tokens.begin() + 1, tokens.end() });
    size_t consumed = 0;
    Args global = parse_args(tokens, global_options(), true, &consumed);
    if (global.has("version")) {
        ctx.out.line("rant-cli " RANT_CLI_VERSION " (Rant " + std::string(rant::version()) + ")");
        return 0;
    }
    if (consumed == tokens.size()) {
        ctx.out.line(main_help());
        return 0;
    }
    const std::string& name = tokens[consumed];
    const Command* cmd = find(name);
    if (!cmd) throw UsageError("unknown command `" + name + "`, run `rant --help` for the list");

    /* a command's own option wins over a global one of the same name, as lib install --version */
    std::vector<OptionSpec> specs = cmd->options;
    specs.insert(specs.end(), global_options().begin(), global_options().end());
    ctx.args = parse_args({ tokens.begin() + consumed + 1, tokens.end() }, specs);
    ctx.args.merge(global);

    if (ctx.args.has("help") && !cmd->own_help) {
        ctx.out.line(command_help(*cmd));
        return 0;
    }
    ctx.yes = ctx.args.has("yes");
    ctx.json = ctx.args.has("json");
    if (auto d = ctx.args.get("domain")) ctx.domain = parse_domain(*d);
    return cmd->run(ctx);
}

}
