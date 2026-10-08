group {
  include "base" {}

  node "arm" {
    type = "demo/sensor"
    args = "--task move"
  }
}
