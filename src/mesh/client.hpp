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
    bool        here = false;    /* on this machine: its ip is one of ours */

    std::string host() const;    /* the ip alone */
};

/* Names under @rant/ are Rant's own: the builtin entities and the CLI's nodes. */
bool is_internal(std::string_view name);

/* A name like "0x1234abcd" stands in for an entity whose details have not arrived yet. */
bool is_placeholder(std::string_view name);

/* How long a settle waits by default, long enough to hear from every peer on a LAN. */
inline constexpr std::chrono::milliseconds full_settle{ 600 };

/* One short lived observer node per command. It opens with details fetched for every
 * peer, so a single bounded settle answers who is there and what they offer. */
class Client {
public:
    explicit Client(uint16_t domain);

    rant::Node& node() { return node_; }

    /* Solicits the mesh and waits until every peer that answered is known in detail, or
     * until limit. Nothing answering costs the whole limit. What it saw goes to the snapshot. */
    void settle(std::chrono::milliseconds limit = full_settle);

    std::vector<Peer>         peers() const;        /* active, internal ones hidden, by name */
    std::vector<rant::Entity> entities() const;     /* the mesh folded, builtins hidden, by name */
    std::vector<rant::Entity> entities_of(uint32_t peer) const;

    /* Each peer's process id from its @rant/meta answer. A peer that does not answer within
     * limit is left out. */
    std::map<uint32_t, uint64_t> pids(const std::vector<uint32_t>& peers,
                                      std::chrono::milliseconds limit = std::chrono::milliseconds(500));

private:
    rant::Node node_;
    uint16_t   domain_;
};

const char* kind_name(rant::EntityKind k);    /* "topic", "var", "fn", "task" */

}
