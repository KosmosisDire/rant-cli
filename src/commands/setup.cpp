#include <algorithm>
#include <map>

#include "agents/skill.hpp"
#include "app/failure.hpp"
#include "commands/commands.hpp"
#include "complete/complete.hpp"
#include "complete/shells.hpp"

namespace commands {

static std::string shell_names() {
    std::string s;
    for (auto& sh : complete::shells()) s += (s.empty() ? "" : ", ") + std::string(sh.name);
    return s;
}

static const complete::Shell& named(const std::string& name) {
    const complete::Shell* sh = complete::find_shell(name);
    if (!sh) throw app::UsageError("unknown shell `" + name + "`, rant knows " + shell_names());
    return *sh;
}

/* Writes the skill once per folder of skills, naming the agents that read it there.
 * Returns false when one failed. */
static bool install_skill(app::Context& ctx, const std::vector<agents::Agent>& found) {
    std::map<agents::fs::path, std::string> readers;
    for (auto& a : found) readers[a.skills] += (readers[a.skills].empty() ? "" : ", ") + a.name;
    bool ok = true;
    for (auto& [skills, names] : readers) {
        try {
            ctx.out.line(names + ": the rant skill in " + agents::install(skills).u8string());
        } catch (const app::Failure& e) {
            ctx.out.warn(names + ": " + e.what());
            ok = false;
        }
    }
    return ok;
}

/* The shells named, every one found when none is, and the skill only when `skill` is one
 * of the words, since it goes into the agents' own folders. */
static int run(app::Context& ctx) {
    auto& w = ctx.args.words;
    if (ctx.args.has("print")) {
        if (w.size() != 1) throw app::UsageError("--print takes one shell, one of " + shell_names());
        ctx.out.line(std::string(named(w[0]).hook));
        return 0;
    }
    bool skill = std::find(w.begin(), w.end(), "skill") != w.end();
    std::vector<const complete::Shell*> chosen;
    for (auto& name : w)
        if (name != "skill") chosen.push_back(&named(name));
    if (w.empty()) chosen = complete::detected();
    std::vector<agents::Agent> found = agents::detected();
    if (skill && found.empty()) throw app::Failure("no AI coding agent found");
    if (!skill && chosen.empty()) throw app::Failure("found no shell to set up, name one of " + shell_names());

    bool failed = false;
    for (auto* sh : chosen) {
        try {
            ctx.out.line(std::string(sh->name) + ": tab completion " + complete::install(*sh));
        } catch (const app::Failure& e) {
            ctx.out.warn(std::string(sh->name) + ": " + e.what());
            failed = true;
        }
    }
    if (!chosen.empty()) ctx.out.note("open a new shell to use it");
    if (skill) {
        failed |= !install_skill(ctx, found);
    } else if (!found.empty()) {
        std::string names;
        for (auto& a : found) names += (names.empty() ? "" : ", ") + a.name;
        ctx.out.note("`rant setup skill` teaches " + names + " to use Rant");
    }
    return failed ? 1 : 0;
}

static complete::Candidates complete_words(complete::Request&) {
    std::vector<std::string> out = { "skill" };
    for (auto& sh : complete::shells()) out.emplace_back(sh.name);
    return { out };
}

app::Command setup() {
    app::Command c{ "setup", "[shell...] [skill]",
                    "set up tab completion for your shells, or the named ones, and with skill the rant skill for your AI coding agents",
                    app::Section::Setup,
                    { { "print", 0, "", "print the hook for one shell instead of installing it" } }, run };
    c.complete = complete_words;
    return c;
}

}
