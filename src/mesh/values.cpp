#include "mesh/values.hpp"

#include <cstring>
#include <vector>

namespace mesh {

namespace d = rant::detail;

static std::string hex(const uint8_t* p, size_t n) {
    static const char digits[] = "0123456789abcdef";
    std::string s = "0x";
    for (size_t i = 0; i < n; i++) {
        s += digits[p[i] >> 4];
        s += digits[p[i] & 15];
    }
    return s;
}

static std::string text(const uint8_t* p, size_t n) { return std::string(reinterpret_cast<const char*>(p), n); }

/* One fixed scalar, little endian as on the wire. */
static json scalar(const uint8_t* p, uint8_t kind) {
    uint64_t u = 0;
    uint32_t size = d::rant_schema_scalar_size(static_cast<d::RantSchemaTypeKind>(kind));
    for (uint32_t i = 0; i < size; i++) u |= (uint64_t)p[i] << (8 * i);
    switch (kind) {
    case d::RANT_U8: case d::RANT_U16: case d::RANT_U32: case d::RANT_U64: return u;
    case d::RANT_I8:  return (int64_t)(int8_t)u;
    case d::RANT_I16: return (int64_t)(int16_t)u;
    case d::RANT_I32: return (int64_t)(int32_t)u;
    case d::RANT_I64: return (int64_t)u;
    case d::RANT_F32: { uint32_t b = (uint32_t)u; float f; std::memcpy(&f, &b, 4); return (double)f; }
    case d::RANT_F64: { double f; std::memcpy(&f, &u, 8); return f; }
    case d::RANT_BOOL: return u != 0;
    default: return nullptr;
    }
}

static json tagged(const d::RantValue& v);

static json map_body(d::RantBytes body) {
    json obj = json::object();
    uint16_t n = d::rant_map_count(body);
    for (uint16_t i = 0; i < n; i++) {
        d::RantString key;
        d::RantValue v;
        if (!d::rant_map_at(body, i, &key, &v)) break;
        obj[std::string(key.data, key.len)] = tagged(v);
    }
    return obj;
}

/* A value inside a map, which carries its own kind. */
static json tagged(const d::RantValue& v) {
    switch (v.kind) {
    case d::RANT_U8: case d::RANT_U16: case d::RANT_U32: case d::RANT_U64: return v.v.u;
    case d::RANT_I8: case d::RANT_I16: case d::RANT_I32: case d::RANT_I64: return v.v.i;
    case d::RANT_F32: case d::RANT_F64: return v.v.f;
    case d::RANT_BOOL: return v.v.u != 0;
    case d::RANT_STR: case d::RANT_VSTR: return text(v.bytes.data, v.bytes.len);
    case d::RANT_MAP: return map_body(v.bytes);
    case d::RANT_ARR: case d::RANT_VARR: {
        json arr = json::array();
        uint16_t n = d::rant_map_array_count(v.bytes);
        for (uint16_t i = 0; i < n; i++) {
            d::RantValue e;
            if (!d::rant_map_array_at(v.bytes, i, &e)) break;
            arr.push_back(tagged(e));
        }
        return arr;
    }
    default: return nullptr;
    }
}

/* Walks the flat depth first field table. elems holds one index per enclosing struct
 * array, which is how a member of the k-th element is addressed. */
class Walk {
public:
    Walk(rant::Bytes data, const d::RantSchema* s)
        : msg_(d::rant_bytes(data.data(), data.size())), s_(s), n_(d::rant_schema_field_count(s)) {}

    json root() {
        uint16_t i = 0;
        d::RantSchemaFieldInfo f;
        if (n_ == 0 || !d::rant_schema_field_at(s_, 0, &f)) return nullptr;
        if (f.name.len == 0) return field(i);
        return members(i, 0);
    }

private:
    d::RantSchemaFieldInfo info(uint16_t i) const {
        d::RantSchemaFieldInfo f{};
        d::rant_schema_field_at(s_, i, &f);
        return f;
    }

    /* The struct members from i on at depth, stopping at the first shallower field. */
    json members(uint16_t& i, uint16_t depth) {
        json obj = json::object();
        while (i < n_) {
            d::RantSchemaFieldInfo f = info(i);
            if (f.depth < depth) break;
            std::string name(f.name.data, f.name.len);
            obj[name] = field(i);
        }
        return obj;
    }

    /* The value of field i, leaving i past it and everything nested under it. */
    json field(uint16_t& i) {
        d::RantSchemaFieldInfo f = info(i);
        uint16_t at = i++;
        if (f.kind == d::RANT_STRUCT) return members(i, (uint16_t)(f.depth + 1));
        bool struct_array = (f.kind == d::RANT_ARR || f.kind == d::RANT_VARR) && f.elem == d::RANT_STRUCT;
        if (!struct_array) return leaf(at, f);

        d::RantValue v{};
        d::rant_get_value_at(msg_, s_, at, elems_.data(), (uint16_t)elems_.size(), &v);
        json arr = json::array();
        uint16_t end = i;
        for (uint32_t k = 0; k < v.count; k++) {
            elems_.push_back(k);
            uint16_t j = i;
            arr.push_back(members(j, (uint16_t)(f.depth + 1)));
            end = j;
            elems_.pop_back();
        }
        if (v.count == 0)
            while (end < n_ && info(end).depth > f.depth) end++;
        i = end;
        return arr;
    }

    json leaf(uint16_t at, const d::RantSchemaFieldInfo& f) {
        d::RantValue v{};
        if (!d::rant_get_value_at(msg_, s_, at, elems_.data(), (uint16_t)elems_.size(), &v)) return nullptr;
        switch (f.kind) {
        case d::RANT_STR: case d::RANT_VSTR: return text(v.bytes.data, v.bytes.len);
        case d::RANT_MAP: return map_body(v.bytes);
        case d::RANT_ENUM: {
            d::RantString name = d::rant_enum_name_of(s_, at, v.v.i);
            if (name.data) return std::string(name.data, name.len);
            return v.v.i;
        }
        case d::RANT_ARR: case d::RANT_VARR: {
            json arr = json::array();
            for (uint32_t k = 0; k < v.count && f.elem_size; k++) {
                const uint8_t* p = v.bytes.data + (size_t)k * f.elem_size;
                if (f.elem == d::RANT_STR) {
                    uint16_t len = (uint16_t)(p[0] | (p[1] << 8));
                    if (len > f.elem_size - 2) len = (uint16_t)(f.elem_size - 2);
                    arr.push_back(text(p + 2, len));
                } else {
                    arr.push_back(scalar(p, f.elem));
                }
            }
            return arr;
        }
        case d::RANT_BOOL: return v.v.u != 0;
        case d::RANT_F32: case d::RANT_F64: return v.v.f;
        case d::RANT_I8: case d::RANT_I16: case d::RANT_I32: case d::RANT_I64: return v.v.i;
        default: return v.v.u;
        }
    }

    d::RantBytes              msg_;
    const d::RantSchema*      s_;
    uint16_t                  n_;
    std::vector<uint32_t>     elems_;
};

json to_json(rant::Bytes data, const rant::Schema& schema) {
    if (schema.empty()) return hex(data.data(), data.size());
    return Walk(data, schema.raw()).root();
}

}
