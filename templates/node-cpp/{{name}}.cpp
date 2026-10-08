#include <chrono>
#include <thread>

#include "rant.hpp"

using namespace std::chrono_literals;

int main() {
    rant::Node node("{{name}}");
    auto count = node.publisher<double>("{{name}}/count");
    node.settle();
    for (double i = 0;; i++) {
        count.send(i);
        std::this_thread::sleep_for(1s);
    }
}
