#pragma once

#include <cstdint>
#include <set>
#include <string>
#include <utility>

#include "rant.hpp"

namespace mesh {

class Client;

/* The names last seen on a domain, kept in ~/.rant/cache so completion can answer at once
 * from what an earlier command saw. */
struct Snapshot {
    std::set<std::string>                                nodes;
    std::set<std::pair<std::string, rant::EntityKind>>   entities;

    static Snapshot load(uint16_t domain);

    /* Writes it atomically. A failure only costs a slower completion later. */
    void save(uint16_t domain) const;

    void add(const Client& mesh);
};

}
