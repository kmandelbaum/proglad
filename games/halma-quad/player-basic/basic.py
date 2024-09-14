import fileinput
import sys

GAME_START = 'start'
YOUR_MOVE = 'yourmove'

def main():
    odd_move = True
    print("ready")
    sys.stdout.flush()
    for line in fileinput.input():
        cmd = list(line.strip().split())
        if cmd[0] == GAME_START:
            player = int(cmd[1])
        elif cmd[0] == YOUR_MOVE:
            if player == 1:
                if odd_move:
                    print("move 1 5 1 6")
                else:
                    print("move 1 6 1 5")
            elif player == 2:
                if odd_move:
                    print("move 16 12 16 11")
                else:
                    print("move 16 11 16 12")
            else:
                raise ValueError(f'my player is not 1 or 2 but {player}')
            sys.stdout.flush()
            odd_move = not odd_move


if __name__ == '__main__':
    main()
