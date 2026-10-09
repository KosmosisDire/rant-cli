import rant
from rant.types import Pose2D


def main():
    """Prints every pose the talker publishes."""
    node = rant.Node("listener")
    pose = node.subscriber("pose", Pose2D)
    while True:
        p = pose.take(None)
        print(f"x {p.position.x:.2f}  y {p.position.y:.2f}  heading {p.angle:.2f} rad")


if __name__ == "__main__":
    main()
