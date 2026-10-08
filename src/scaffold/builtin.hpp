#pragma once

#include <cstddef>
#include <vector>

namespace scaffold {

/* One file of a built in template, its path starting with the template's name. */
struct BuiltinFile {
    const char*          path;    /* "package-cpp/CMakeLists.txt" */
    const unsigned char* data;
    size_t               size;
};

/* Every file under templates/, compiled in by cmake/embed_templates.cmake. */
const std::vector<BuiltinFile>& builtin_files();

}
