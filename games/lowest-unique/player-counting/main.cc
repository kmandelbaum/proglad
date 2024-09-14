#include <iostream>
#include <map>
#include <vector>
#include <string>

int main() {
  std::cout << "ready\n" << std::flush;
  int n, p, m;
  int best = 1;
  std::vector<int> win_counts;
  std::vector<int> cnt;
  std::vector<int> mv;
  while (true) {
    std::string cmd;
    std::cin >> cmd;
    if (cmd == "start") {
      std::cin >> n >> p >> m;
      cnt.resize(m + 1);
      mv.resize(n + 1);
      win_counts.resize(m + 1);
    } else if (cmd == "yourmove") {
      std::cout << best << std::endl << std::flush;
    } else if (cmd == "move") {
      int winner;
      std::cin >> winner;
      for (int i = 1; i <= m; i++) {
        cnt[i] = 0;
      }
      for (int i = 1; i <= n; i++) {
        std::cin >> mv[i];
        if (i != p) {
          cnt[mv[i]]++;
        }
      }
      for (int i = 1; i <= m; i++) {
        bool found = false;
        for (int j = i + 1; j <= m; j++) {
          if (cnt[j] > 0 && cnt[j] < cnt[i] + 1) {
            found = true;
            break;
          }
        }
        if (!found) {
          win_counts[i] += 1;
          break;
        }
      }
      int best_win_count = 0;
      for (int i = 1; i <= m; i++) {
        if (win_counts[i] > best_win_count) {
          best = i;
          best_win_count = win_counts[i];
        }
      }
    }
  }
  return 0;
}
