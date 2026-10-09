#include <cstring>
#include <regex>

#include "app/failure.hpp"
#include "mesh/values.hpp"

namespace mesh {

namespace d = rant::detail;

static std::string text_of(const json& v) {
    return v.is_string() ? v.get<std::string>() : v.dump(-1, ' ', false, json::error_handler_t::replace);
}

static std::string shown(const std::string& path) { return path.empty() ? "the value" : "field `" + path + "`"; }

static std::string join(const std::string& path, const std::string& key) { return path.empty() ? key : path + "." + key; }

/* Writes one scalar of kind into p, little endian as on the wire. */
static bool put_scalar(uint8_t* p, uint8_t kind, const json& v) {
    uint32_t size = d::rant_schema_scalar_size(static_cast<d::RantSchemaTypeKind>(kind));
    uint64_t bits = 0;
    if (kind == d::RANT_F32 || kind == d::RANT_F64) {
        if (!v.is_number()) return false;
        if (kind == d::RANT_F32) {
            float f = v.get<float>();
            uint32_t b;
            std::memcpy(&b, &f, 4);
            bits = b;
        } else {
            double f = v.get<double>();
            std::memcpy(&bits, &f, 8);
        }
    } else if (kind == d::RANT_BOOL) {
        if (!v.is_boolean()) return false;
        bits = v.get<bool>();
    } else {
        if (!v.is_number_integer()) return false;
        bits = v.is_number_unsigned() ? v.get<uint64_t>() : (uint64_t)v.get<int64_t>();
    }
    for (uint32_t i = 0; i < size; i++) p[i] = (uint8_t)(bits >> (8 * i));
    return true;
}

/* A `map` field's body: a self describing tree, so any JSON object fits. */
static void map_entries(d::RantMapWriter& w, const json& obj);

static void map_value(d::RantMapWriter& w, const char* key, const json& v) {
    if (v.is_boolean()) d::rant_map_put_bool(&w, key, v.get<bool>());
    else if (v.is_number_unsigned()) d::rant_map_put_uint(&w, key, v.get<uint64_t>());
    else if (v.is_number_integer()) d::rant_map_put_int(&w, key, v.get<int64_t>());
    else if (v.is_number()) d::rant_map_put_f64(&w, key, v.get<double>());
    else if (v.is_string()) {
        const std::string& s = v.get_ref<const std::string&>();
        d::rant_map_put_string(&w, key, d::rant_string(s.data(), s.size()));
    } else if (v.is_object()) {
        d::rant_map_open_map(&w, key);
        map_entries(w, v);
        d::rant_map_close(&w);
    } else if (v.is_array()) {
        d::rant_map_open_array(&w, key);
        for (auto& e : v) map_value(w, nullptr, e);
        d::rant_map_close(&w);
    }
}

static void map_entries(d::RantMapWriter& w, const json& obj) {
    for (auto& [k, v] : obj.items()) map_value(w, k.c_str(), v);
}

static std::vector<uint8_t> map_body(const json& obj) {
    for (size_t cap = 256; cap <= (64u << 20); cap *= 4) {
        std::vector<uint8_t> buf(cap);
        d::RantMapWriter w = d::rant_map_begin(buf.data(), buf.size());
        map_entries(w, obj);
        uint32_t n = d::rant_map_finish(&w);
        if (n) {
            buf.resize(n);
            return buf;
        }
    }
    throw app::Failure("the value is too big to send");
}

/* Builds one message field by field, through the paths the schema resolves. */
class Encoder {
public:
    explicit Encoder(const rant::Schema& s) : s_(s), b_(s) {}

    void set(const std::string& path, const json& v) {
        grow_arrays(path);
        int idx = s_.field_index(path);
        if (idx < 0) fail_unknown(path);
        d::RantSchemaFieldInfo f{};
        d::rant_schema_field_at(s_.raw(), (uint16_t)idx, &f);
        const char* p = path.c_str();
        bool arr = f.kind == d::RANT_ARR || f.kind == d::RANT_VARR;

        if (f.kind == d::RANT_STRUCT) {
            if (!v.is_object()) wrong(path, v, "an object of its fields");
            for (auto& [k, val] : v.items()) set(join(path, k), val);
        } else if (arr && f.elem == d::RANT_STRUCT) {
            if (!v.is_array()) wrong(path, v, "a list");
            size_t n = sized(path, f, v.size());
            for (size_t i = 0; i < n; i++) {
                if (!v[i].is_object()) wrong(path + "[" + std::to_string(i) + "]", v[i], "an object of its fields");
                for (auto& [k, val] : v[i].items()) set(path + "[" + std::to_string(i) + "]." + k, val);
            }
        } else if (arr && f.elem == d::RANT_STR) {
            if (!v.is_array()) wrong(path, v, "a list of text");
            size_t n = sized(path, f, v.size());
            for (size_t i = 0; i < n; i++) b_.set_string_at(p, (uint16_t)i, text_of(v[i]));
        } else if (arr) {
            if (!v.is_array()) wrong(path, v, "a list of numbers");
            if (f.kind == d::RANT_ARR && v.size() > f.count) wrong(path, v, "at most " + std::to_string(f.count) + " items");
            std::vector<uint8_t> raw(v.size() * f.elem_size);
            for (size_t i = 0; i < v.size(); i++)
                if (!put_scalar(raw.data() + i * f.elem_size, f.elem, v[i])) wrong(path + "[" + std::to_string(i) + "]", v[i], kind_word(f.elem));
            b_.set_array(p, rant::Bytes(raw.data(), raw.size()));
        } else if (f.kind == d::RANT_STR || f.kind == d::RANT_VSTR) {
            b_.set_string(p, text_of(v));
        } else if (f.kind == d::RANT_ENUM) {
            if (v.is_number_integer()) b_.set_enum(p, v.get<int64_t>());
            else b_.set_enum(p, text_of(v));
            if (!b_.ok()) wrong(path, v, "one of " + options(idx));
        } else if (f.kind == d::RANT_MAP) {
            if (!v.is_object()) wrong(path, v, "an object");
            auto body = map_body(v);
            b_.set_map(p, rant::Bytes(body.data(), body.size()));
        } else if (f.kind == d::RANT_BOOL) {
            if (!v.is_boolean()) wrong(path, v, "true or false");
            b_.set_bool(p, v.get<bool>());
        } else if (f.kind == d::RANT_F32 || f.kind == d::RANT_F64) {
            if (!v.is_number()) wrong(path, v, "a number");
            b_.set_f64(p, v.get<double>());
        } else {
            if (!v.is_number_integer()) wrong(path, v, "a whole number");
            bool is_unsigned = f.kind <= d::RANT_U64;
            if (is_unsigned && !v.is_number_unsigned()) wrong(path, v, "a whole number from 0 up");
            if (is_unsigned) b_.set_uint(p, v.get<uint64_t>());
            else b_.set_int(p, v.get<int64_t>());
        }
        if (!b_.ok()) wrong(path, v, "a value that fits it");
    }

    std::vector<uint8_t> bytes() const {
        rant::Bytes b = b_.bytes();
        return { b.data(), b.data() + b.size() };
    }

private:
    [[noreturn]] void wrong(const std::string& path, const json& v, const std::string& want) const {
        throw app::Failure(shown(path) + " takes " + want + ", not " + text_of(v));
    }

    static std::string kind_word(uint8_t kind) {
        if (kind == d::RANT_BOOL) return "true or false";
        if (kind == d::RANT_F32 || kind == d::RANT_F64) return "a number";
        return "a whole number";
    }

    std::string options(int idx) const {
        std::string out;
        for (uint16_t i = 0; i < d::rant_schema_enum_count(s_.raw(), (uint16_t)idx); i++) {
            int64_t value;
            d::RantString name;
            if (d::rant_schema_enum_variant(s_.raw(), (uint16_t)idx, i, &value, &name))
                out += (out.empty() ? "" : ", ") + std::string(name.data, name.len);
        }
        return out;
    }

    /* The field names one level under path, for the error about a name it does not have. */
    [[noreturn]] void fail_unknown(const std::string& path) const {
        size_t dot = path.find_last_of('.');
        std::string parent = dot == std::string::npos ? "" : path.substr(0, dot);
        int pidx = parent.empty() ? -1 : s_.field_index(parent);
        uint16_t depth = 0;
        if (pidx >= 0) {
            d::RantSchemaFieldInfo pf{};
            d::rant_schema_field_at(s_.raw(), (uint16_t)pidx, &pf);
            depth = (uint16_t)(pf.depth + 1);
        }
        std::string known;
        uint16_t n = d::rant_schema_field_count(s_.raw());
        for (uint16_t i = (uint16_t)(pidx + 1); i < n; i++) {
            d::RantSchemaFieldInfo f{};
            d::rant_schema_field_at(s_.raw(), i, &f);
            if (f.depth < depth) break;
            if (f.depth == depth && f.name.len) known += (known.empty() ? "" : ", ") + std::string(f.name.data, f.name.len);
        }
        throw app::Failure("there is no field `" + path + "`" + (known.empty() ? "" : ", the fields are " + known));
    }

    /* A list's length: a variable array is resized to it, a fixed one must hold it. */
    size_t sized(const std::string& path, const d::RantSchemaFieldInfo& f, size_t n) {
        if (f.kind == d::RANT_VARR) b_.set_array_count(path.c_str(), (uint32_t)n);
        else if (n > f.count) throw app::Failure(shown(path) + " holds at most " + std::to_string(f.count) + " items");
        return n;
    }

    /* pts[2].x on a variable array of two grows it to three first. */
    void grow_arrays(const std::string& path) {
        static const std::regex index("\\[([0-9]+)\\]");
        for (std::sregex_iterator it(path.begin(), path.end(), index), end; it != end; ++it) {
            std::string prefix = path.substr(0, (size_t)it->position());
            uint32_t want = (uint32_t)std::stoul((*it)[1]) + 1;
            int idx = s_.field_index(prefix);
            if (idx < 0) continue;
            d::RantSchemaFieldInfo f{};
            d::rant_schema_field_at(s_.raw(), (uint16_t)idx, &f);
            rant::Bytes now = b_.bytes();
            if (f.kind == d::RANT_VARR && d::rant_get_array_count(d::rant_bytes(now.data(), now.size()), s_.raw(), prefix.c_str()) < want)
                b_.set_array_count(prefix.c_str(), want);
        }
    }

    const rant::Schema&  s_;
    rant::MessageBuilder b_;
};

std::vector<uint8_t> encode(const Assignments& values, const rant::Schema& schema) {
    if (schema.empty()) {
        if (values.size() != 1 || !values[0].first.empty())
            throw app::Failure("the entity has no type, so it takes one value, sent as text");
        std::string t = text_of(values[0].second);
        return { t.begin(), t.end() };
    }
    Encoder e(schema);
    for (auto& [path, v] : values) {
        if (path.empty() && v.is_object()) {
            d::RantSchemaFieldInfo root{};
            d::rant_schema_field_at(schema.raw(), 0, &root);
            if (root.name.len) {    /* a struct: each key is a field */
                for (auto& [k, val] : v.items()) e.set(k, val);
                continue;
            }
        }
        e.set(path, v);
    }
    return e.bytes();
}

}
