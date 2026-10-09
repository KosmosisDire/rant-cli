#include <cstdio>

#include "rant.hpp"

/* Prints every pose the talker publishes. */
int main() {
    rant::Node node("listener");
    auto pose = node.subscriber<rant::types::Pose2D>("pose");
    while (auto p = pose.take(rant::forever)) {
        std::printf("x %.2f  y %.2f  heading %.2f rad\n", p->position.x, p->position.y, p->angle);
        std::fflush(stdout);    /* the log file keeps up */
    }
}
