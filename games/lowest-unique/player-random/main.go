package main

import (
    "bufio"
    "fmt"
    "math/rand"
    "os"
    "strconv"
)

func get(scanner *bufio.Scanner) string {
  if !scanner.Scan() {
    return ""
  }
  return scanner.Text()
}

func main() {
    scanner := bufio.NewScanner(os.Stdin)
    scanner.Split(bufio.ScanWords)
    f := bufio.NewWriter(os.Stdout)
    fmt.Fprintf(f, "ready\n")
    f.Flush()
    var options int
    var players int
    L:
    for {
      switch (get(scanner)) {
      case "":
        break L
      case "start":
        players, _ = strconv.Atoi(get(scanner))
        strconv.Atoi(get(scanner)) // this player
        options, _ = strconv.Atoi(get(scanner))
      case "yourmove":
        fmt.Fprintf(f, "%d\n", 1 + rand.Intn(options))
        f.Flush()
      case "move":
        for i := 0; i <= players; i++ {
          get(scanner)
        }
      default:
        break
      }
    }
}
