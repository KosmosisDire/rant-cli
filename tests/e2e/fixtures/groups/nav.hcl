group {
  description = "Drive around"

  param "speed" {
    type    = float
    default = 1
  }

  include "base" {
    expose = true
  }

  node "planner" {
    type = "demo/sensor"
    args = ["--pub", "plan", "--fn", "speed_${param.speed}"]
  }
}
