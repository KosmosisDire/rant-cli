import itertools
import time

import rant


def main():
    node = rant.Node("{{name}}")
    count = node.publisher("{{name}}/count", float)
    for i in itertools.count():
        count.send(float(i))
        time.sleep(1)


if __name__ == "__main__":
    main()
