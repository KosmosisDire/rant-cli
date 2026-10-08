package {
  name = "tools"

  node "echoer" {
    run = "cmake -E echo hello"
  }
}
