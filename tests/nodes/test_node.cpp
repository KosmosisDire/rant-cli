/* A Rant node for the end to end tests. Every flag adds one entity:
 *
 *   test_node [--name N] [--domain D] [--pub TOPIC] [--var NAME] [--fn NAME] [--task NAME]
 *             [--life SECONDS] [--ignore-stop]
 *
 * --pub sends Reading { seq, value } at 20 Hz. --var owns an f64 starting at 1.5. --fn
 * doubles an f64. --task counts to its u32 request, one step each 100 ms, cancellable.
 * Ctrl-C, Ctrl-Break and SIGTERM print "stopped" and exit 0, unless --ignore-stop.
 * Without --domain the domain comes from TEST_NODE_DOMAIN, which a node started by rant
 * inherits from the test that ran rant. */

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#include "rant.hpp"

struct Reading {
    uint32_t seq;
    double   value;
};
RANT_SCHEMA(Reading, seq, value);

static std::atomic<bool> stop{ false };
static bool ignore_stop = false;

#ifdef _WIN32
static BOOL WINAPI on_ctrl(DWORD) {
    if (!ignore_stop) stop = true;
    return TRUE;
}
#else
static void on_signal(int) {
    if (!ignore_stop) stop = true;
}
#endif

int main(int argc, char** argv) {
    std::string name = "test_node", pub, var, fn, task;
    const char* env_domain = std::getenv("TEST_NODE_DOMAIN");
    uint16_t domain = env_domain ? (uint16_t)std::atoi(env_domain) : 0;
    double life = 0;
    for (int i = 1; i < argc; i++) {
        std::string a = argv[i];
        auto next = [&]() -> std::string {
            if (i + 1 >= argc) { std::fprintf(stderr, "%s needs a value\n", a.c_str()); std::exit(2); }
            return argv[++i];
        };
        if (a == "--name") name = next();
        else if (a == "--domain") domain = (uint16_t)std::atoi(next().c_str());
        else if (a == "--pub") pub = next();
        else if (a == "--var") var = next();
        else if (a == "--fn") fn = next();
        else if (a == "--task") task = next();
        else if (a == "--life") life = std::atof(next().c_str());
        else if (a == "--ignore-stop") ignore_stop = true;
        else { std::fprintf(stderr, "unknown argument %s\n", a.c_str()); return 2; }
    }

#ifdef _WIN32
    SetConsoleCtrlHandler(on_ctrl, TRUE);
#else
    std::signal(SIGTERM, on_signal);
    std::signal(SIGINT, on_signal);
#endif

    rant::NodeOptions o;
    o.domain = domain;
    o.max_topics = 16;
    rant::Node node(name, o);

    std::mutex workers_mu;
    std::vector<std::thread> workers;
    rant::Publisher<Reading> publisher;
    if (!pub.empty()) publisher = node.publisher<Reading>(pub);
    rant::VariableDefinition<double> variable;
    if (!var.empty()) {
        rant::VariableOptions<double> vo;
        vo.initial = 1.5;
        variable = node.variable_definition<double>(var, vo);
    }
    rant::FunctionDefinition<double, double> function;
    if (!fn.empty()) function = node.function_definition<double, double>(fn, [](const double& x) { return x * 2; });
    rant::TaskDefinition<uint32_t, uint32_t, uint32_t> counter;
    if (!task.empty()) {
        counter = node.task_definition<uint32_t, uint32_t, uint32_t>(task,
            [&](const uint32_t& n, rant::TaskRequest<uint32_t, uint32_t>& t) {
                std::lock_guard<std::mutex> lock(workers_mu);
                workers.emplace_back([n, p = t.defer()]() mutable {
                    for (uint32_t i = 1; i <= n; i++) {
                        std::this_thread::sleep_for(std::chrono::milliseconds(100));
                        if (p.cancelled()) { p.complete_cancelled("cancelled", i); return; }
                        p.progress(i);
                    }
                    p.complete(n);
                });
            });
    }

    std::printf("ready %s\n", name.c_str());
    std::fflush(stdout);
    auto start = std::chrono::steady_clock::now();
    uint32_t seq = 0;
    while (!stop) {
        if (life > 0 && std::chrono::steady_clock::now() - start > std::chrono::duration<double>(life)) break;
        if (publisher.valid()) publisher.send({ seq, seq * 0.5 }), seq++;
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    std::lock_guard<std::mutex> lock(workers_mu);
    for (auto& w : workers) w.join();
    std::printf("stopped\n");
    std::fflush(stdout);
    return 0;
}
