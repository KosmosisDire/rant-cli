#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "config/config.hpp"

/* The logs nodes write: <logs>/<name>.log, the run before kept as <name>.log.1. */
namespace run {

/* The last n lines of a log, fewer when it is shorter, none when it cannot be read. */
std::vector<std::string> tail(const config::fs::path& log, size_t n);

/* Every node with a log in the logs folder, by name, a prefixed one as left/lidar. */
std::vector<std::string> logged_nodes(const config::fs::path& logs);

/* A log followed from its current end. A restart moves the log aside and starts a new one,
 * which the follower tells by the file's identity and reads from its start. The file is
 * never held open, so the move can happen. */
class Follower {
public:
    explicit Follower(config::fs::path log);

    /* The complete lines written since the last call. */
    std::vector<std::string> poll();

private:
    config::fs::path path_;
    std::string      id_;
    uint64_t         offset_ = 0;
    std::string      partial_;
};

}
