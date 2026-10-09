import math
import time

import rant
from rant.types import Double2, Pose2D


def main():
    """Drives a point around the unit circle and publishes where it is."""
    node = rant.Node("talker")
    pose = node.publisher("pose", Pose2D)
    t = 0.0
    while True:
        pose.send(Pose2D(position=Double2(x=math.cos(t), y=math.sin(t)), angle=t + math.pi / 2))
        t += 0.1
        time.sleep(0.5)


if __name__ == "__main__":
    main()
