# The test copies the test node to bin/. Every node dies on its own within a minute, so a
# failed test never leaves one running for long.
package {
  name = "demo"

  node "sensor" {
    run = ["./bin/test_node", "--life", "60"]
  }
}
