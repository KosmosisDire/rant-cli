#pragma once

/* A minimal test harness. Each test file is one executable of named cases:
 *
 *   TEST(splits_words) { CHECK_EQ(split("a b").size(), 2u); }
 *
 * Run it with no arguments for every case, or name cases to run only those. */

#include <cstdio>
#include <sstream>
#include <string>
#include <vector>

namespace check {

struct Case {
    const char* name;
    void (*fn)();
};

inline std::vector<Case>& cases() {
    static std::vector<Case> list;
    return list;
}

inline int& failures() {
    static int n = 0;
    return n;
}

struct Register {
    Register(const char* name, void (*fn)()) { cases().push_back({ name, fn }); }
};

inline void fail(const char* file, int line, const std::string& what) {
    failures()++;
    std::fprintf(stderr, "%s:%d: check failed: %s\n", file, line, what.c_str());
}

template <class T>
std::string show(const T& v) {
    std::ostringstream s;
    s << v;
    return s.str();
}

}

#define TEST(name)                                                  \
    static void name();                                             \
    static const check::Register register_##name(#name, name);     \
    static void name()

#define CHECK(cond)                                                 \
    do {                                                            \
        if (!(cond)) check::fail(__FILE__, __LINE__, #cond);        \
    } while (0)

#define CHECK_EQ(a, b)                                                                          \
    do {                                                                                        \
        const auto check_a = (a);                                                               \
        const auto check_b = (b);                                                               \
        if (!(check_a == check_b))                                                              \
            check::fail(__FILE__, __LINE__,                                                     \
                        std::string(#a " == " #b ", got ") + check::show(check_a) + " and " +   \
                            check::show(check_b));                                              \
    } while (0)

#define CHECK_THROWS(expr)                                                       \
    do {                                                                         \
        bool check_threw = false;                                                \
        try { (void)(expr); } catch (...) { check_threw = true; }                \
        if (!check_threw) check::fail(__FILE__, __LINE__, #expr " did not throw"); \
    } while (0)
