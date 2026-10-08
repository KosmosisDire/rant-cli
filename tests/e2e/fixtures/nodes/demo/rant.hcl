# The test copies the test node to bin/. Every node dies on its own within a minute, so a
# failed test never leaves one running for long.
package {
  name = "demo"

  node "chatty" {
    run = ["./bin/test_node", "--life", "60", "--pub", "chatter"]
  }

  node "stubborn" {
    run = ["./bin/test_node", "--life", "60", "--ignore-stop"]
  }

  node "brief" {
    run = ["./bin/test_node", "--life", "1"]
  }
}
