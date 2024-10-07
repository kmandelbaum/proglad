import fileinput
import random
import sys

GAME_START = "start"
YOUR_MOVE = "yourmove"

def main():
    print("ready")
    sys.stdout.flush()
    for line in fileinput.input():
        cmd = list(line.strip().split())
        if not cmd:
            break
        if cmd[0] == GAME_START:
            N, p, M, R = map(int, cmd[1:])
        elif cmd[0] == YOUR_MOVE:
            print(random.choice(range(1, M + 1)))
            sys.stdout.flush()

if __name__ == "__main__":
    main()
