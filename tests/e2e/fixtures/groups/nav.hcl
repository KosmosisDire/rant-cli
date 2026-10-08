group {
  description = "Drive around"

  param "speed" {
    type    = float
    default = 1
  }

  include "base" {
    expose = true
  }

  node "demo/sensor" {
    name = "planner"
    args = ["--pub", "plan", "--fn", "speed_${param.speed}"]
  }
}
