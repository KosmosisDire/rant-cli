#include "complete/complete.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <set>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

#include "app/command.hpp"
#include "commands/commands.hpp"
#include "mesh/client.hpp"
#include "run/nodes.hpp"

namespace complete {

Request::Request(app::Context& ctx, std::vector<std::string> w, std::string p)
    : words(std::move(w)), partial(std::move(p)), ctx_(ctx) {}

const config::Workspace* Request::workspace() {
    if (!opened_) opened_ = config::open(ctx_.cwd, true);
    return opened_->workspace ? &*opened_->workspace : nullptr;
}

std::vector<std::string> Request::node_types() {
    std::vector<std::string> out;
    const config::Workspace* ws = workspace();
    if (!ws) return out;
    std::map<std::string, int> uses;
    for (auto& p : ws->packages)
        for (auto& n : p.nodes) {
            out.push_back(n.ref());
            uses[n.name]++;
        }
    for (auto& n : ws->loose) uses[n.name]++;
    for (auto& [name, count] : uses)
        if (count == 1) out.push_back(name);
    return out;
}

std::vector<std::string> Request::groups() {
    std::vector<std::string> out;
    if (const config::Workspace* ws = workspace())
        for (auto& g : ws->groups) out.push_back(g.name);
    return out;
}

std::vector<std::string> Request::packages() {
    std::vector<std::string> out;
    if (const config::Workspace* ws = workspace())
        for (auto& p : ws->packages) out.push_back(p.name);
    return out;
}

config::GroupInfo Request::group(const std::string& name) { return config::describe_group(ctx_.cwd, name); }

std::vector<std::string> Request::running_nodes() {
    std::vector<std::string> out;
    for (auto& i : run::snapshot(ctx_).instances) out.push_back(i.name);
    return out;
}

std::vector<std::string> Request::running_groups() {
    std::vector<std::string> out;
    for (auto& r : run::snapshot(ctx_).roots)
        if (r.kind == "group") out.push_back(r.name);
    return out;
}

const mesh::Snapshot& Request::mesh() {
    if (!mesh_) {
        try {
            mesh::Client client(ctx_.domain);
            client.settle(mesh_budget);
        } catch (...) {
        }
        mesh_ = mesh::Snapshot::load(ctx_.domain);
    }
    return *mesh_;
}

std::vector<std::string> Request::mesh_nodes() {
    const auto& nodes = mesh().nodes;
    return { nodes.begin(), nodes.end() };
}

std::vector<std::string> Request::entities(std::initializer_list<rant::EntityKind> kinds) {
    std::vector<std::string> out;
    for (auto& [name, kind] : mesh().entities)
        if (kinds.size() == 0 || std::find(kinds.begin(), kinds.end(), kind) != kinds.end()) out.push_back(name);
    return out;
}

std::vector<std::string> params(const config::GroupInfo& group, const std::string& partial,
                                const std::vector<std::string>& given) {
    std::vector<std::string> out;
    size_t eq = partial.find('=');
    if (eq != std::string::npos) {
        std::string key = partial.substr(0, eq);
        for (auto& p : group.params) {
            if (p.name != key) continue;
            std::vector<std::string> values = p.options;
            if (values.empty() && p.type == "bool") values = { "true", "false" };
            for (auto& v : values) out.push_back(key + "=" + v);
        }
        return out;
    }
    for (auto& p : group.params) {
        bool done = std::any_of(given.begin(), given.end(), [&](const std::string& g) { return g.rfind(p.name + "=", 0) == 0; });
        if (!done) out.push_back(p.name + "=");
    }
    return out;
}

std::vector<std::string> narrow(const Candidates& c, const std::string& partial) {
    std::set<std::string> out;
    for (auto& w : c.words) {
        if (w.rfind(partial, 0) != 0) continue;
        if (c.paths) {    /* a leading slash starts the path, it ends no segment */
            size_t slash = w.find('/', std::max<size_t>(partial.size(), 1));
            if (slash != std::string::npos && slash + 1 < w.size()) {
                out.insert(w.substr(0, slash + 1));
                continue;
            }
        }
        out.insert(w);
    }
    return { out.begin(), out.end() };
}

static const app::Command* find_command(const std::string& name) {
    for (auto& c : commands::all())
        if (c.name == name) return &c;
    return nullptr;
}

static const app::OptionSpec* find_option(const std::vector<app::OptionSpec>& specs, const std::string& t) {
    if (t.rfind("--", 0) == 0) {
        std::string name = t.substr(2, t.find('=') == std::string::npos ? std::string::npos : t.find('=') - 2);
        for (auto& s : specs)
            if (s.name == name) return &s;
    } else if (t.size() == 2) {
        for (auto& s : specs)
            if (s.short_name == t[1]) return &s;
    }
    return nullptr;
}

static void set_domain(app::Context& ctx, const std::string& v) {
    char* end = nullptr;
    unsigned long d = std::strtoul(v.c_str(), &end, 10);
    if (!v.empty() && !*end && d <= 65535) ctx.domain = (uint16_t)d;
}

/* The words before the cursor read as the real parse reads them: options and their values
 * set aside, the first word the command. */
struct Line {
    const app::Command*      command = nullptr;
    bool                     unknown = false;     /* the first word names no command */
    bool                     on_value = false;    /* the cursor is on an option's value */
    std::vector<std::string> words;
    std::vector<app::OptionSpec> specs = app::global_options();
};

static Line read_line(app::Context& ctx, const std::vector<std::string>& before) {
    Line l;
    bool options_done = false;
    for (size_t i = 0; i < before.size(); i++) {
        const std::string& t = before[i];
        bool option = !options_done && t.size() > 1 && t[0] == '-' && !app::looks_like_number(t);
        if (!option) {
            if (l.command || l.unknown) {
                l.words.push_back(t);
            } else if ((l.command = find_command(t))) {
                l.specs.insert(l.specs.end(), l.command->options.begin(), l.command->options.end());
            } else {
                l.unknown = true;
            }
            continue;
        }
        if (t == "--") {
            options_done = true;
            continue;
        }
        const app::OptionSpec* spec = find_option(l.specs, t);
        if (!spec || spec->value_name.empty()) continue;
        size_t eq = t.find('=');
        std::string value;
        if (t.rfind("--", 0) == 0 && eq != std::string::npos) value = t.substr(eq + 1);
        else if (i + 1 < before.size()) value = before[++i];
        else l.on_value = true;
        if (spec->name == "domain") set_domain(ctx, value);
    }
    return l;
}

static std::vector<std::string> answer(app::Context& ctx, const std::vector<std::string>& before, const std::string& partial) {
    Line l = read_line(ctx, before);
    if (l.on_value || l.unknown) return {};
    Candidates c;
    if (!partial.empty() && partial[0] == '-' && !app::looks_like_number(partial)) {
        for (auto& s : l.specs) c.words.push_back("--" + std::string(s.name));
    } else if (!l.command) {
        for (auto& cmd : commands::all()) c.words.emplace_back(cmd.name);
    } else if (l.command->complete) {
        Request r(ctx, l.words, partial);
        c = l.command->complete(r);
    }
    return narrow(c, partial);
}

int run(app::Context& ctx, const std::vector<std::string>& tokens) {
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);    /* a bare \n, which every shell splits on */
#endif
    std::string partial;
    std::vector<std::string> before = tokens;
    if (!before.empty() && before[0].rfind("--cur=", 0) == 0) {
        partial = before[0].substr(6);
        before.erase(before.begin());
    }
    std::vector<std::string> out;
    try {
        out = answer(ctx, before, partial);
    } catch (...) {
    }
    for (auto& w : out) {
        std::fputs(w.c_str(), stdout);
        std::fputc('\n', stdout);
    }
    return 0;
}

}
