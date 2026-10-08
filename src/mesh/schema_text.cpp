#include "mesh/schema_text.hpp"

#include <algorithm>
#include <set>
#include <vector>

namespace mesh {

namespace d = rant::detail;
using ui::Style;

static const char* scalar_name(uint8_t kind) {
    switch (kind) {
    case d::RANT_U8: return "u8";
    case d::RANT_U16: return "u16";
    case d::RANT_U32: return "u32";
    case d::RANT_U64: return "u64";
    case d::RANT_I8: return "i8";
    case d::RANT_I16: return "i16";
    case d::RANT_I32: return "i32";
    case d::RANT_I64: return "i64";
    case d::RANT_F32: return "f32";
    case d::RANT_F64: return "f64";
    case d::RANT_BOOL: return "bool";
    default: return "?";
    }
}

/* Walks the flat field table, where a struct's members follow it one level deeper. */
class Printer {
public:
    Printer(const d::RantSchema* s, const ui::Output* out) : s_(s), out_(out), n_(d::rant_schema_field_count(s)) {}

    std::string print() {
        std::vector<std::string> root;
        d::RantString name = d::rant_schema_name(s_);
        d::RantSchemaFieldInfo first = info(0);
        if (n_ == 0) {
            root.push_back(paint(Style::Faint, "{}"));
        } else if (name.len && standard(name)) {
            root.push_back(paint(Style::Accent, std::string(name.data, name.len)));
        } else if (first.name.len == 0) {
            root = type_lines(0, 0);    /* a bare root, such as f64 or u8[] */
        } else {
            uint16_t at = 0;
            std::string head = name.len ? paint(Style::Accent, std::string(name.data, name.len)) + " " : "";
            if (name.len) defined_.insert(std::string(name.data, name.len));
            root.push_back(head + paint(Style::Faint, "{"));
            for (auto& l : members(at, 0, 2)) root.push_back(l);
            root.push_back(paint(Style::Faint, "}"));
        }
        std::string out = join(root);
        for (auto& def : defs_) out += "\n\n" + def;
        return out;
    }

private:
    d::RantSchemaFieldInfo info(uint16_t i) const {
        d::RantSchemaFieldInfo f{};
        d::rant_schema_field_at(s_, i, &f);
        return f;
    }

    std::string paint(Style st, const std::string& t) const { return out_ ? out_->paint(st, t) : t; }

    static bool standard(d::RantString name) { return d::rant_std_by_name(name) != d::RANT_STD_NONE; }

    static std::string join(const std::vector<std::string>& lines) {
        std::string s;
        for (auto& l : lines) s += (s.empty() ? "" : "\n") + l;
        return s;
    }

    /* The direct members of a struct, from j on at depth, names aligned. Leaves j past
     * everything nested under them. */
    std::vector<std::string> members(uint16_t& j, uint16_t depth, size_t indent) {
        std::vector<uint16_t> direct;
        while (j < n_ && info(j).depth >= depth) {
            if (info(j).depth == depth) direct.push_back(j);
            j++;
        }
        size_t widest = 0;
        for (auto i : direct) widest = std::max(widest, (size_t)info(i).name.len);
        std::vector<std::string> lines;
        for (auto i : direct) {
            d::RantSchemaFieldInfo f = info(i);
            std::string name(f.name.data, f.name.len);
            auto type = type_lines(i, indent);
            lines.push_back(std::string(indent, ' ') + name + paint(Style::Faint, ":") +
                            std::string(widest - name.size() + 1, ' ') + type[0]);
            lines.insert(lines.end(), type.begin() + 1, type.end());
        }
        return lines;
    }

    /* A named type other than a struct: a standard one stays a name, a user one is defined
     * once below as Name = type. */
    std::string named(const std::string& name, const std::vector<std::string>& type) {
        if (!standard(d::rant_string(name.data(), name.size())) && !defined_.count(name)) {
            defined_.insert(name);
            defs_.push_back(paint(Style::Accent, name) + " " + paint(Style::Faint, "=") + " " + join(type));
        }
        return paint(Style::Accent, name);
    }

    std::string enum_text(uint16_t i, const d::RantSchemaFieldInfo& f) {
        std::string s = paint(Style::Amber, "enum") + paint(Style::Faint, "<") + paint(Style::Amber, scalar_name(f.elem)) +
                        paint(Style::Faint, "> {");
        for (uint16_t k = 0; k < d::rant_schema_enum_count(s_, i); k++) {
            int64_t value = 0;
            d::RantString name;
            if (!d::rant_schema_enum_variant(s_, i, k, &value, &name)) continue;
            s += (k ? paint(Style::Faint, ",") + " " : " ") + paint(Style::GreenHi, std::string(name.data, name.len));
            if (value != k) s += " " + paint(Style::Faint, "=") + " " + std::to_string(value);
        }
        return s + " " + paint(Style::Faint, "}");
    }

    /* The struct a field or an array element opens: named, or inline over several lines. */
    std::vector<std::string> struct_type(uint16_t i, d::RantString type_name, size_t indent) {
        uint16_t at = (uint16_t)(i + 1);
        uint16_t depth = (uint16_t)(info(i).depth + 1);
        if (type_name.len) {
            std::string name(type_name.data, type_name.len);
            if (standard(type_name) || defined_.count(name)) return { paint(Style::Accent, name) };
            defined_.insert(name);
            size_t slot = defs_.size();    /* the definition goes before the types it uses */
            defs_.emplace_back();
            std::vector<std::string> lines{ paint(Style::Accent, name) + " " + paint(Style::Faint, "{") };
            for (auto& l : members(at, depth, 2)) lines.push_back(l);
            lines.push_back(paint(Style::Faint, "}"));
            defs_[slot] = join(lines);
            return { paint(Style::Accent, name) };
        }
        std::vector<std::string> lines{ paint(Style::Faint, "{") };
        for (auto& l : members(at, depth, indent + 2)) lines.push_back(l);
        lines.push_back(std::string(indent, ' ') + paint(Style::Faint, "}"));
        return lines;
    }

    std::vector<std::string> type_lines(uint16_t i, size_t indent) {
        d::RantSchemaFieldInfo f = info(i);
        auto alias = [&](const std::string& base) {
            if (!f.type_name.len) return base;
            return named(std::string(f.type_name.data, f.type_name.len), { base });
        };
        switch (f.kind) {
        case d::RANT_STRUCT: return struct_type(i, f.type_name, indent);
        case d::RANT_STR:
            return { alias(paint(Style::Amber, "string") + paint(Style::Faint, "<") + std::to_string(f.str_cap) + paint(Style::Faint, ">")) };
        case d::RANT_VSTR: return { alias(paint(Style::Amber, "string")) };
        case d::RANT_MAP:  return { alias(paint(Style::Amber, "map")) };
        case d::RANT_ENUM: return { alias(enum_text(i, f)) };
        case d::RANT_ARR:
        case d::RANT_VARR: {
            std::string suffix = paint(Style::Faint, "[") + (f.kind == d::RANT_ARR ? std::to_string(f.count) : "") +
                                 paint(Style::Faint, "]");
            std::vector<std::string> elem;
            if (f.elem == d::RANT_STRUCT) {
                elem = struct_type(i, f.elem_name, indent);
            } else {
                std::string base = f.elem == d::RANT_STR
                                       ? paint(Style::Amber, "string") + paint(Style::Faint, "<") + std::to_string(f.str_cap) + paint(Style::Faint, ">")
                                       : paint(Style::Amber, scalar_name(f.elem));
                elem = { f.elem_name.len ? named(std::string(f.elem_name.data, f.elem_name.len), { base }) : base };
            }
            elem.back() += suffix;
            if (f.type_name.len) return { named(std::string(f.type_name.data, f.type_name.len), elem) };
            return elem;
        }
        default: return { alias(paint(Style::Amber, scalar_name(f.kind))) };
        }
    }

    const d::RantSchema*     s_;
    const ui::Output*        out_;
    uint16_t                 n_;
    std::vector<std::string> defs_;
    std::set<std::string>    defined_;
};

std::string schema_text(const rant::Schema& s, const ui::Output* out) {
    if (s.empty()) return out ? out->paint(Style::Faint, "untyped") : "untyped";
    return Printer(s.raw(), out).print();
}

}
