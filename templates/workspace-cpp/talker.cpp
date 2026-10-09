#include <chrono>
#include <cmath>
#include <thread>

#include "rant.hpp"

using namespace std::chrono_literals;

constexpr double quarter_turn = 1.5707963267948966;

/* Drives a point around the unit circle and publishes where it is. */
int main() {
    rant::Node node("talker");
    auto pose = node.publisher<rant::types::Pose2D>("pose");
    node.settle();
    for (double t = 0;; t += 0.1) {
        pose.send({ { std::cos(t), std::sin(t) }, t + quarter_turn });
        std::this_thread::sleep_for(500ms);
    }
}
