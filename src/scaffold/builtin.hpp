#pragma once

#include <cstddef>
#include <vector>

namespace scaffold {

/* One file under templates/, a preset's or the agent skill's, its path starting with the folder. */
struct BuiltinFile {
    const char*          path;    /* "package-cpp/CMakeLists.txt" */
    const unsigned char* data;
    size_t               size;
};

/* Every file under templates/, compiled in by cmake/embed_templates.cmake. */
const std::vector<BuiltinFile>& builtin_files();

}
