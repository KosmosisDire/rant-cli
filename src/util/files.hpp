#pragma once

#include <filesystem>
#include <system_error>

namespace util {

/* fs::rename, which Windows refuses while another process holds either side open, as a virus
 * scan holds fresh files for a moment. A refused rename is tried again for up to 3 s. */
void rename(const std::filesystem::path& from, const std::filesystem::path& to, std::error_code& ec);

}
