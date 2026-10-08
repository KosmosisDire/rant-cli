#include "mesh/client.hpp"

#include <algorithm>
#include <condition_variable>
#include <mutex>
#include <thread>

namespace mesh {

bool is_internal(std::string_view name) { return name.rfind("@rant/", 0) == 0; }

const char* kind_name(rant::EntityKind k) {
    switch (k) {
    case rant::EntityKind::Topic:    return "topic";
    case rant::EntityKind::Variable: return "var";
    case rant::EntityKind::Function: return "fn";
    case rant::EntityKind::Task:     return "task";
    }
    return "?";
}

static rant::NodeOptions observer_options(uint16_t domain) {
    rant::NodeOptions o;
    o.domain = domain;
    o.fetch_details = true;
    o.disable_logs = true;
    o.max_peers = 256;
    o.max_topics = 16;
    return o;
}

Client::Client(uint16_t domain) : node_("@rant/cli", observer_options(domain)) {
    node_.on_event([](const rant::Event&) {});    /* a peer's schema clash is not the CLI's error */
}

/* A placeholder name ("0x1234abcd") means the entity's details have not arrived yet. */
static bool details_pending(const std::vector<rant::Entity>& es) {
    for (auto& e : es)
        if (e.name.size() == 10 && e.name.rfind("0x", 0) == 0) return true;
    return false;
}

void Client::settle(std::chrono::milliseconds limit) {
    auto deadline = std::chrono::steady_clock::now() + limit;
    node_.settle(limit);
    auto r = node_.reflection();
    while (details_pending(r.mesh()) && std::chrono::steady_clock::now() < deadline)
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
}

std::vector<Peer> Client::peers() const {
    std::vector<Peer> out;
    for (auto& p : node_.reflection().peers())
        if (p.active && !is_internal(p.name)) out.push_back({ p.id, p.name, p.address });
    std::sort(out.begin(), out.end(), [](const Peer& a, const Peer& b) { return a.name < b.name; });
    return out;
}

static void keep_public(std::vector<rant::Entity>& es) {
    es.erase(std::remove_if(es.begin(), es.end(), [](const rant::Entity& e) { return is_internal(e.name); }),
             es.end());
    std::sort(es.begin(), es.end(), [](const rant::Entity& a, const rant::Entity& b) {
        return a.name != b.name ? a.name < b.name : a.kind < b.kind;
    });
}

std::vector<rant::Entity> Client::entities() const {
    auto es = node_.reflection().mesh();
    keep_public(es);
    return es;
}

std::vector<rant::Entity> Client::entities_of(uint32_t peer) const {
    auto es = node_.reflection().entities(peer);
    keep_public(es);
    return es;
}

std::map<uint32_t, uint64_t> Client::pids(const std::vector<uint32_t>& peers, std::chrono::milliseconds limit) {
    struct Shared {
        std::mutex mu;
        std::condition_variable cv;
        std::map<uint32_t, uint64_t> pids;
        size_t answered = 0;
    };
    auto shared = std::make_shared<Shared>();
    size_t asked = 0;
    auto r = node_.reflection();
    for (uint32_t peer : peers) {
        auto st = r.meta_async(peer, [shared, peer](const rant::MetaSnapshot& m) {
            std::lock_guard<std::mutex> lock(shared->mu);
            if (m.valid && m.proc.have) shared->pids[peer] = m.proc.pid;
            shared->answered++;
            shared->cv.notify_all();
        }, RANT_META_PROC);
        if (st == rant::SendStatus::Ok) asked++;
    }
    std::unique_lock<std::mutex> lock(shared->mu);
    shared->cv.wait_for(lock, limit, [&] { return shared->answered >= asked; });
    return shared->pids;
}

}
