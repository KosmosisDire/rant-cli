#pragma once

#include <chrono>
#include <cstdint>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "rant.hpp"

namespace mesh {

/* A live node on the mesh. */
struct Peer {
    uint32_t    id = 0;
    std::string name;
    std::string address;    /* "ip:port" */
};

/* Names under @rant/ are Rant's own: the builtin entities and the CLI's nodes. */
bool is_internal(std::string_view name);

/* One short lived observer node per command. It opens with details fetched for every
 * peer, so a single bounded settle answers who is there and what they offer. */
class Client {
public:
    explicit Client(uint16_t domain);

    rant::Node& node() { return node_; }

    /* Solicits the mesh and waits until every peer that answered is known in detail, or
     * until limit. Nothing answering costs the whole limit. */
    void settle(std::chrono::milliseconds limit = std::chrono::milliseconds(600));

    std::vector<Peer>         peers() const;        /* active, internal ones hidden, by name */
    std::vector<rant::Entity> entities() const;     /* the mesh folded, builtins hidden, by name */
    std::vector<rant::Entity> entities_of(uint32_t peer) const;

    /* Each peer's process id from its @rant/meta answer. A peer that does not answer within
     * limit is left out. */
    std::map<uint32_t, uint64_t> pids(const std::vector<uint32_t>& peers,
                                      std::chrono::milliseconds limit = std::chrono::milliseconds(500));

private:
    rant::Node node_;
};

const char* kind_name(rant::EntityKind k);    /* "topic", "var", "fn", "task" */

}
